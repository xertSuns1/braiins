// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

use ii_logging::macros::*;

use ii_bitcoin::HashTrait;

use crate::client;
use crate::job::{self, Bitcoin as _};
use crate::node::{self, ClientStats as _};
use crate::stats;
use crate::work;

use bosminer_macros::ClientNode;

use async_trait::async_trait;
use futures::lock::Mutex;
use ii_async_compat::{futures, tokio};
use tokio::prelude::*;

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;

use ii_stratum::v2::framing::codec::Framing;
use ii_stratum::v2::messages::{
    NewMiningJob, OpenStandardMiningChannel, OpenStandardMiningChannelError,
    OpenStandardMiningChannelSuccess, SetNewPrevHash, SetTarget, SetupConnection,
    SetupConnectionError, SetupConnectionSuccess, SubmitShares, SubmitSharesError,
    SubmitSharesSuccess,
};
use ii_stratum::v2::types::DeviceInfo;
use ii_stratum::v2::types::*;
use ii_stratum::v2::{Handler, Protocol};
use ii_wire::{Connection, ConnectionRx, ConnectionTx, Message};

use std::collections::HashMap;

// TODO: move it to the stratum crate
const VERSION_MASK: u32 = 0x1fffe000;

#[derive(Debug, Clone)]
struct StratumJob {
    client: Arc<StratumClient>,
    id: u32,
    channel_id: u32,
    version: u32,
    prev_hash: ii_bitcoin::DHash,
    merkle_root: ii_bitcoin::DHash,
    time: u32,
    max_time: u32,
    bits: u32,
    target: ii_bitcoin::Target,
}

impl StratumJob {
    pub fn new(
        client: Arc<StratumClient>,
        job_msg: &NewMiningJob,
        prevhash_msg: &SetNewPrevHash,
        target: ii_bitcoin::Target,
    ) -> Self {
        Self {
            client,
            id: job_msg.job_id,
            channel_id: job_msg.channel_id,
            version: job_msg.version,
            prev_hash: ii_bitcoin::DHash::from_slice(prevhash_msg.prev_hash.as_ref())
                .expect("BUG: Stratum: incorrect size of prev hash"),
            merkle_root: ii_bitcoin::DHash::from_slice(job_msg.merkle_root.as_ref())
                .expect("BUG: Stratum: incorrect size of merkle root"),
            time: prevhash_msg.min_ntime,
            max_time: prevhash_msg.min_ntime + prevhash_msg.max_ntime_offset as u32,
            bits: prevhash_msg.nbits,
            target,
        }
    }

    /// Check if stratum job is valid
    fn sanity_check(&self) -> bool {
        let mut valid = true;
        if let Err(msg) = ii_bitcoin::Target::from_compact(self.bits()) {
            error!("Stratum: invalid job's nBits ({})", msg);
            valid = false;
        }
        valid
    }
}

impl job::Bitcoin for StratumJob {
    fn origin(&self) -> node::DynInfo {
        self.client.clone()
    }

    fn version(&self) -> u32 {
        self.version
    }

    fn version_mask(&self) -> u32 {
        VERSION_MASK
    }

    fn previous_hash(&self) -> &ii_bitcoin::DHash {
        &self.prev_hash
    }

    fn merkle_root(&self) -> &ii_bitcoin::DHash {
        &self.merkle_root
    }

    fn time(&self) -> u32 {
        self.time
    }

    fn max_time(&self) -> u32 {
        self.max_time
    }

    fn bits(&self) -> u32 {
        self.bits
    }

    fn target(&self) -> ii_bitcoin::Target {
        self.target
    }

    fn is_valid(&self) -> bool {
        // TODO: currently there is no easy way to detect the job is valid -> we have to check
        //  its presence in the registry. The inequality below was possible in the previous
        //  iteration of the protocol
        // self.block_height >= self.current_block_height.load(Ordering::Relaxed)
        true
    }
}

/// Queue that contains pairs of solution and its assigned sequence number. It is our responsibility
/// to keep the sequence number monotonic so that we as a stratum V2 client can easily process bulk
/// acknowledgements. The sequence number type has been selected as u32 to match
/// up with the protocol.
type SolutionQueue = Arc<Mutex<VecDeque<(work::Solution, u32)>>>;

#[derive(Debug, ClientNode)]
struct StratumClient {
    pub descriptor: client::Descriptor,
    #[member_client_stats]
    client_stats: stats::BasicClient,
}

impl StratumClient {
    pub fn new(descriptor: client::Descriptor) -> Self {
        Self {
            descriptor,
            client_stats: Default::default(),
        }
    }
}

impl node::Client for StratumClient {
    fn url(&self) -> String {
        self.descriptor.url.clone()
    }

    fn user(&self) -> String {
        self.descriptor.user.clone()
    }
}

impl fmt::Display for StratumClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}@{} ({})",
            self.descriptor.url, self.descriptor.user, self.descriptor.protocol
        )
    }
}

struct StratumEventHandler {
    client: Arc<StratumClient>,
    status: Result<(), ()>,
    job_sender: job::Sender,
    all_jobs: HashMap<u32, NewMiningJob>,
    current_prevhash_msg: Option<SetNewPrevHash>,
    /// Mining target for the next job that is to be solved
    current_target: ii_bitcoin::Target,
    solutions: SolutionQueue,
}

impl StratumEventHandler {
    pub fn new(
        job_sender: job::Sender,
        client: Arc<StratumClient>,
        solutions: SolutionQueue,
    ) -> Self {
        Self {
            client,
            status: Err(()),
            job_sender,
            all_jobs: Default::default(),
            current_prevhash_msg: None,
            current_target: Default::default(),
            solutions,
        }
    }

    /// Convert new mining job message into StratumJob and send it down the line for solving.
    ///
    /// * `job_msg` - job message used as a base for the StratumJob
    pub fn update_job(&mut self, job_msg: &NewMiningJob) {
        let job = StratumJob::new(
            self.client.clone(),
            job_msg,
            self.current_prevhash_msg.as_ref().expect("no prevhash"),
            self.current_target,
        );
        // TODO: move it to the job sender
        if job.sanity_check() {
            // send only valid jobs
            self.client.client_stats().valid_jobs().inc();
            self.job_sender.send(Arc::new(job));
        } else {
            self.client.client_stats().invalid_jobs().inc();
        }
    }

    pub fn update_target(&mut self, value: Uint256Bytes) {
        let new_target: ii_bitcoin::Target = value.into();
        info!(
            "Stratum: changing target to {} diff={}",
            new_target,
            new_target.get_difficulty()
        );
        self.current_target = new_target;
    }

    async fn process_accepted_shares(&self, success_msg: &SubmitSharesSuccess) {
        let now = std::time::Instant::now();
        while let Some((solution, seq_num)) = self.solutions.lock().await.pop_front() {
            info!(
                "Stratum: accepted solution #{} with nonce={:08x}",
                seq_num,
                solution.nonce()
            );
            self.client
                .client_stats
                .accepted
                .account_solution(&solution.job_target(), now)
                .await;
            if success_msg.last_seq_num == seq_num {
                // all accepted solutions have been found
                return;
            }
        }
        warn!(
            "Stratum: last accepted solution #{} hasn't been found!",
            success_msg.last_seq_num
        );
    }

    async fn process_rejected_shares(&self, error_msg: &SubmitSharesError) {
        let now = std::time::Instant::now();
        while let Some((solution, seq_num)) = self.solutions.lock().await.pop_front() {
            if error_msg.seq_num == seq_num {
                info!(
                    "Stratum: rejected solution #{} with nonce={:08x}!",
                    seq_num,
                    solution.nonce()
                );
                self.client
                    .client_stats
                    .rejected
                    .account_solution(&solution.job_target(), now)
                    .await;
                // the rejected solution has been found
                return;
            } else {
                // TODO: this is currently not according to stratum V2 specification
                // preceding solutions are treated as accepted
                info!(
                    "Stratum: accepted solution #{} with nonce={}",
                    seq_num,
                    solution.nonce()
                );
                self.client
                    .client_stats
                    .accepted
                    .account_solution(&solution.job_target(), now)
                    .await;
                warn!(
                    "Stratum: the solution #{} precedes rejected solution #{}!",
                    seq_num, error_msg.seq_num
                );
                warn!(
                    "Stratum: the solution #{} is treated as an accepted one",
                    seq_num
                );
            }
        }
        warn!(
            "Stratum: rejected solution #{} hasn't been found!",
            error_msg.seq_num
        );
    }
}

#[async_trait]
impl Handler for StratumEventHandler {
    // The rules for prevhash/mining job pairing are (currently) as follows:
    //  - when mining job comes
    //      - store it (by id)
    //      - start mining it if it doesn't have the future_job flag set
    //  - when prevhash message comes
    //      - replace it
    //      - start mining the job it references (by job id)
    //      - flush all other jobs

    async fn visit_new_mining_job(&mut self, _msg: &Message<Protocol>, job_msg: &NewMiningJob) {
        // all jobs since last `prevmsg` have to be stored in job table
        self.all_jobs.insert(job_msg.job_id, job_msg.clone());
        // TODO: close connection when maximal capacity of `all_jobs` has been reached

        // When not marked as future job, we can start mining on it right away
        if !job_msg.future_job {
            self.update_job(job_msg);
        }
    }

    async fn visit_set_new_prev_hash(
        &mut self,
        _msg: &Message<Protocol>,
        prevhash_msg: &SetNewPrevHash,
    ) {
        self.current_prevhash_msg.replace(prevhash_msg.clone());

        // find the future job with ID referenced in prevhash_msg
        let (_, mut future_job_msg) = self
            .all_jobs
            .remove_entry(&prevhash_msg.job_id)
            .expect("requested job ID not found");

        // remove all other jobs (they are now invalid)
        self.all_jobs.retain(|_, _| true);
        // turn the job into an immediate job
        future_job_msg.future_job = false;
        // reinsert the job
        self.all_jobs
            .insert(future_job_msg.job_id, future_job_msg.clone());

        // and start immediately solving it
        self.update_job(&future_job_msg);
    }

    async fn visit_set_target(&mut self, _msg: &Message<Protocol>, target_msg: &SetTarget) {
        self.update_target(target_msg.max_target);
    }

    async fn visit_submit_shares_success(
        &mut self,
        _msg: &Message<Protocol>,
        success_msg: &SubmitSharesSuccess,
    ) {
        self.process_accepted_shares(success_msg).await;
    }

    async fn visit_submit_shares_error(
        &mut self,
        _msg: &Message<Protocol>,
        error_msg: &SubmitSharesError,
    ) {
        self.process_rejected_shares(error_msg).await;
    }

    async fn visit_setup_connection_success(
        &mut self,
        _msg: &Message<Protocol>,
        _success_msg: &SetupConnectionSuccess,
    ) {
        self.status = Ok(());
    }

    async fn visit_setup_connection_error(
        &mut self,
        _msg: &Message<Protocol>,
        _error_msg: &SetupConnectionError,
    ) {
        self.status = Err(());
    }

    async fn visit_open_standard_mining_channel_success(
        &mut self,
        _msg: &Message<Protocol>,
        success_msg: &OpenStandardMiningChannelSuccess,
    ) {
        self.update_target(success_msg.target);
        self.status = Ok(());
    }

    async fn visit_open_standard_mining_channel_error(
        &mut self,
        _msg: &Message<Protocol>,
        _error_msg: &OpenStandardMiningChannelError,
    ) {
        self.status = Err(());
    }
}

struct StratumSolutionHandler {
    connection_tx: ConnectionTx<Framing>,
    job_solution: job::SolutionReceiver,
    solutions: SolutionQueue,
    seq_num: u32,
}

impl StratumSolutionHandler {
    fn new(
        connection_tx: ConnectionTx<Framing>,
        job_solution: job::SolutionReceiver,
        solutions: SolutionQueue,
    ) -> Self {
        Self {
            connection_tx,
            job_solution,
            solutions,
            seq_num: 0,
        }
    }

    async fn process_solution(&mut self, solution: work::Solution) {
        let job: &StratumJob = solution.job();

        let seq_num = self.seq_num;
        self.seq_num = self.seq_num.wrapping_add(1);

        let share_msg = SubmitShares {
            channel_id: job.channel_id,
            seq_num,
            job_id: job.id,
            nonce: solution.nonce(),
            ntime: solution.time(),
            version: solution.version(),
        };
        // store solution with sequence number for future server acknowledge
        self.solutions.lock().await.push_back((solution, seq_num));
        // send solutions back to the stratum server
        self.connection_tx
            .send_msg(share_msg)
            .await
            .expect("Cannot send submit to stratum server");
        // the response is handled in a separate task
    }

    async fn run(mut self) {
        while let Some(solution) = self.job_solution.receive().await {
            self.process_solution(solution).await;
        }
    }
}

async fn setup_mining_connection<'a>(
    connection: &'a mut Connection<Framing>,
    event_handler: &'a mut StratumEventHandler,
    endpoint_hostname: String,
    endpoint_port: usize,
) -> Result<(), ()> {
    let setup_msg = SetupConnection {
        max_version: 0,
        min_version: 0,
        flags: 0,
        // TODO-DOC: Pubkey spec not finalized yet
        expected_pubkey: PubKey::new(),
        endpoint_host: Str0_255::from_string(endpoint_hostname),
        endpoint_port: endpoint_port as u16,
        device: DeviceInfo {
            vendor: "Braiins".try_into()?,
            hw_rev: "1".try_into()?,
            fw_ver: "Braiins OS 2019-06-05".try_into()?,
            dev_id: "xyz".try_into()?,
        },
    };
    connection
        .send_msg(setup_msg)
        .await
        .expect("Cannot send stratum setup mining connection");
    let response_msg = connection
        .next()
        .await
        .expect("Cannot receive response for stratum setup mining connection")
        .unwrap();
    event_handler.status = Err(());
    response_msg.accept(event_handler).await;
    event_handler.status
}

async fn open_channel<'a>(
    connection: &'a mut Connection<Framing>,
    event_handler: &'a mut StratumEventHandler,
    user: String,
) -> Result<(), ()> {
    let channel_msg = OpenStandardMiningChannel {
        req_id: 10,
        user: user.try_into()?,
        nominal_hashrate: 1e9,
        // Maximum bitcoin target is 0xffff << 208 (= difficulty 1 share)
        max_target: ii_bitcoin::Target::default().into(),
    };
    connection
        .send_msg(channel_msg)
        .await
        .expect("Cannot send stratum open channel");
    let response_msg = connection
        .next()
        .await
        .expect("Cannot receive response for stratum open channel")
        .unwrap();
    event_handler.status = Err(());
    response_msg.accept(event_handler).await;
    event_handler.status
}

async fn event_handler_task(
    mut connection_rx: ConnectionRx<Framing>,
    mut event_handler: StratumEventHandler,
) {
    while let Some(msg) = connection_rx.next().await {
        let msg = msg.unwrap();
        msg.accept(&mut event_handler).await;
    }
}

async fn init(job_solver: job::Solver, client: Arc<StratumClient>) {
    let (job_sender, job_solution) = job_solver.split();
    let solutions = Arc::new(Mutex::new(VecDeque::new()));
    let mut event_handler = StratumEventHandler::new(job_sender, client.clone(), solutions.clone());

    let mut connection = Connection::<Framing>::connect(&client.descriptor.socket_addr)
        .await
        .expect("Cannot connect to stratum server");

    setup_mining_connection(
        &mut connection,
        &mut event_handler,
        client.descriptor.url.clone(),
        client.descriptor.socket_addr.port() as usize,
    )
    .await
    .expect("Cannot setup stratum mining connection");
    open_channel(
        &mut connection,
        &mut event_handler,
        client.descriptor.user.clone(),
    )
    .await
    .expect("Cannot open stratum channel");

    let (connection_rx, connection_tx) = connection.split();

    // run event handler in a separate task
    tokio::spawn(event_handler_task(connection_rx, event_handler));
    StratumSolutionHandler::new(connection_tx, job_solution, solutions)
        .run()
        .await;
}

pub fn run(job_solver: job::Solver, descriptor: client::Descriptor) -> Arc<dyn node::Client> {
    let client = Arc::new(StratumClient::new(descriptor));
    tokio::spawn(init(job_solver, client.clone()));
    client
}
