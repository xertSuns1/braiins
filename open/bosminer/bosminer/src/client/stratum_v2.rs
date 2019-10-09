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
use crate::job;
use crate::node;
use crate::work;

use ii_async_compat::tokio;
use tokio::prelude::*;

use std::sync::Arc;

use ii_stratum::v2::framing::codec::Framing;
use ii_stratum::v2::messages::{
    NewMiningJob, OpenStandardMiningChannel, OpenStandardMiningChannelError,
    OpenStandardMiningChannelSuccess, SetNewPrevHash, SetTarget, SetupConnection,
    SetupConnectionError, SetupConnectionSuccess, SubmitShares, SubmitSharesSuccess,
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
    descriptor: Arc<client::Descriptor>,
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
        descriptor: Arc<client::Descriptor>,
        job_msg: &NewMiningJob,
        prevhash_msg: &SetNewPrevHash,
        target: ii_bitcoin::Target,
    ) -> Self {
        Self {
            descriptor,
            id: job_msg.job_id,
            channel_id: job_msg.channel_id,
            version: job_msg.version,
            prev_hash: ii_bitcoin::DHash::from_slice(prevhash_msg.prev_hash.as_ref()).unwrap(),
            merkle_root: ii_bitcoin::DHash::from_slice(job_msg.merkle_root.as_ref()).unwrap(),
            time: prevhash_msg.min_ntime,
            max_time: prevhash_msg.min_ntime + prevhash_msg.max_ntime_offset as u32,
            bits: prevhash_msg.nbits,
            target,
        }
    }
}

impl job::Bitcoin for StratumJob {
    fn origin(&self) -> Arc<dyn node::Info> {
        self.descriptor.clone()
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

struct StratumEventHandler {
    descriptor: Arc<client::Descriptor>,
    status: Result<(), ()>,
    job_sender: job::Sender,
    all_jobs: HashMap<u32, NewMiningJob>,
    current_prevhash_msg: Option<SetNewPrevHash>,
    current_target: ii_bitcoin::Target,
}

impl StratumEventHandler {
    pub fn new(job_sender: job::Sender, descriptor: Arc<client::Descriptor>) -> Self {
        Self {
            descriptor,
            status: Err(()),
            job_sender,
            all_jobs: Default::default(),
            current_prevhash_msg: None,
            current_target: Default::default(),
        }
    }
    pub fn update_job(&mut self, job_msg: &NewMiningJob) {
        let job = StratumJob::new(
            self.descriptor.clone(),
            job_msg,
            self.current_prevhash_msg.as_ref().expect("no prevhash"),
            self.current_target,
        );
        self.job_sender.send(Arc::new(job));
    }

    pub fn update_target(&mut self, value: Uint256Bytes) {
        let new_target: ii_bitcoin::Target = value.into();
        info!("changing target to {:?}", new_target);
        self.current_target = new_target;
    }
}

impl Handler for StratumEventHandler {
    // The rules for prevhash/mining job pairing are (currently) as follows:
    //  - when mining job comes
    //      - store it (by id)
    //      - start mining it if it doesn't have the future_job flag set
    //  - when prevhash message comes
    //      - replace it
    //      - start mining the job it references (by job id)
    //      - flush all other jobs

    fn visit_new_mining_job(&mut self, _msg: &Message<Protocol>, job_msg: &NewMiningJob) {
        // all jobs since last `prevmsg` have to be stored in job table
        self.all_jobs.insert(job_msg.job_id, job_msg.clone());
        // TODO: close connection when maximal capacity of `all_jobs` has been reached

        // When not marked as future job, we can start mining on it right away
        if !job_msg.future_job {
            self.update_job(job_msg);
        }
    }

    fn visit_set_new_prev_hash(&mut self, _msg: &Message<Protocol>, prevhash_msg: &SetNewPrevHash) {
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

    fn visit_set_target(&mut self, _msg: &Message<Protocol>, target_msg: &SetTarget) {
        self.update_target(target_msg.max_target);
    }

    fn visit_setup_connection_success(
        &mut self,
        _msg: &Message<Protocol>,
        _success_msg: &SetupConnectionSuccess,
    ) {
        self.status = Ok(());
    }

    fn visit_setup_connection_error(
        &mut self,
        _msg: &Message<Protocol>,
        _error_msg: &SetupConnectionError,
    ) {
        self.status = Err(());
    }

    fn visit_open_standard_mining_channel_success(
        &mut self,
        _msg: &Message<Protocol>,
        success_msg: &OpenStandardMiningChannelSuccess,
    ) {
        self.update_target(success_msg.target);
        self.status = Ok(());
    }

    fn visit_open_standard_mining_channel_error(
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
    seq_num: u32,
}

impl StratumSolutionHandler {
    fn new(connection_tx: ConnectionTx<Framing>, job_solution: job::SolutionReceiver) -> Self {
        Self {
            connection_tx,
            job_solution,
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
    response_msg.accept(event_handler);
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
    response_msg.accept(event_handler);
    event_handler.status
}

pub struct StringifyV2(Option<String>);

impl StringifyV2 {
    fn new() -> Self {
        Self(None)
    }
    fn print(response_msg: &<Framing as ii_wire::Framing>::Rx) -> String {
        let mut handler = Self::new();
        response_msg.accept(&mut handler);
        handler.0.unwrap_or_else(|| "?unknown?".to_string())
    }
}

impl Handler for StringifyV2 {
    fn visit_setup_connection(
        &mut self,
        _msg: &ii_wire::Message<Protocol>,
        payload: &SetupConnection,
    ) {
        self.0 = Some(format!("{:?}", payload));
    }

    fn visit_setup_connection_success(
        &mut self,
        _msg: &ii_wire::Message<Protocol>,
        payload: &SetupConnectionSuccess,
    ) {
        self.0 = Some(format!("{:?}", payload));
    }

    fn visit_open_standard_mining_channel(
        &mut self,
        _msg: &ii_wire::Message<Protocol>,
        payload: &OpenStandardMiningChannel,
    ) {
        self.0 = Some(format!("{:?}", payload));
    }

    fn visit_open_standard_mining_channel_success(
        &mut self,
        _msg: &ii_wire::Message<Protocol>,
        payload: &OpenStandardMiningChannelSuccess,
    ) {
        self.0 = Some(format!("{:?}", payload));
    }

    fn visit_new_mining_job(&mut self, _msg: &ii_wire::Message<Protocol>, payload: &NewMiningJob) {
        self.0 = Some(format!("{:?}", payload));
    }

    fn visit_set_new_prev_hash(
        &mut self,
        _msg: &ii_wire::Message<Protocol>,
        payload: &SetNewPrevHash,
    ) {
        self.0 = Some(format!("{:?}", payload));
    }

    fn visit_set_target(&mut self, _msg: &ii_wire::Message<Protocol>, payload: &SetTarget) {
        self.0 = Some(format!("{:?}", payload));
    }
    fn visit_submit_shares(&mut self, _msg: &ii_wire::Message<Protocol>, payload: &SubmitShares) {
        self.0 = Some(format!("{:?}", payload));
    }
    fn visit_submit_shares_success(
        &mut self,
        _msg: &ii_wire::Message<Protocol>,
        payload: &SubmitSharesSuccess,
    ) {
        self.0 = Some(format!("{:?}", payload));
    }
}

async fn event_handler_task(
    mut connection_rx: ConnectionRx<Framing>,
    mut event_handler: StratumEventHandler,
) {
    while let Some(msg) = connection_rx.next().await {
        let msg = msg.unwrap();
        trace!("handling message {}", StringifyV2::print(&msg));
        msg.accept(&mut event_handler);
    }
}

pub async fn run(job_solver: job::Solver, descriptor: Arc<client::Descriptor>) {
    let (job_sender, job_solution) = job_solver.split();
    let mut event_handler = StratumEventHandler::new(job_sender, descriptor.clone());

    let mut connection = Connection::<Framing>::connect(&descriptor.socket_addr)
        .await
        .expect("Cannot connect to stratum server");

    setup_mining_connection(
        &mut connection,
        &mut event_handler,
        descriptor.url.clone(),
        descriptor.socket_addr.port() as usize,
    )
    .await
    .expect("Cannot setup stratum mining connection");
    open_channel(&mut connection, &mut event_handler, descriptor.user.clone())
        .await
        .expect("Cannot open stratum channel");

    let (connection_rx, connection_tx) = connection.split();

    // run event handler in a separate task
    tokio::spawn(event_handler_task(connection_rx, event_handler));

    StratumSolutionHandler::new(connection_tx, job_solution)
        .run()
        .await;
}
