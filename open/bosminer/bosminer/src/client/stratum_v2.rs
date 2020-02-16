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

use crate::error;
use crate::hal;
use crate::job;
use crate::node;
use crate::stats;
use crate::sync;
use crate::work;

use failure::ResultExt;

use ii_bitcoin::HashTrait;

use bosminer_config::{ClientDescriptor, ClientProtocol};
use bosminer_macros::ClientNode;

use async_trait::async_trait;
use futures::channel::mpsc;
use futures::lock::Mutex;
use ii_async_compat::prelude::*;
use ii_async_compat::select;

use std::collections::VecDeque;
use std::fmt;
use std::net::ToSocketAddrs;
use std::sync::{Arc, Weak};
use std::time;

use ii_stratum::v2::framing::{Framing, Header};
use ii_stratum::v2::messages::{
    NewMiningJob, OpenStandardMiningChannel, OpenStandardMiningChannelError,
    OpenStandardMiningChannelSuccess, SetNewPrevHash, SetTarget, SetupConnection,
    SetupConnectionError, SetupConnectionSuccess, SubmitSharesError, SubmitSharesStandard,
    SubmitSharesSuccess,
};
use ii_stratum::v2::types::*;
use ii_stratum::v2::{build_message_from_frame, Handler};
use ii_wire::Connection;

use std::collections::HashMap;

// TODO: move it to the stratum crate
const VERSION_MASK: u32 = 0x1fffe000;

const CONNECTION_TIMEOUT: time::Duration = time::Duration::from_secs(5);
const EVENT_TIMEOUT: time::Duration = time::Duration::from_secs(60);

#[derive(Debug)]
pub struct ConnectionDetails {
    pub user: String,
    pub host: String,
    pub port: u16,
}

impl ConnectionDetails {
    pub fn from_descriptor(descriptor: &ClientDescriptor) -> Self {
        Self {
            user: descriptor.user.clone(),
            host: descriptor.host.clone(),
            port: descriptor.port,
        }
    }

    fn get_host_and_port(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone)]
pub struct StratumJob {
    client: Weak<StratumClient>,
    id: u32,
    channel_id: u32,
    version: u32,
    prev_hash: ii_bitcoin::DHash,
    merkle_root: ii_bitcoin::DHash,
    time: u32,
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
            client: Arc::downgrade(&client),
            id: job_msg.job_id,
            channel_id: job_msg.channel_id,
            version: job_msg.version,
            prev_hash: ii_bitcoin::DHash::from_slice(prevhash_msg.prev_hash.as_ref())
                .expect("BUG: Stratum: incorrect size of prev hash"),
            merkle_root: ii_bitcoin::DHash::from_slice(job_msg.merkle_root.as_ref())
                .expect("BUG: Stratum: incorrect size of merkle root"),
            time: prevhash_msg.min_ntime,
            bits: prevhash_msg.nbits,
            target,
        }
    }
}

impl job::Bitcoin for StratumJob {
    fn origin(&self) -> Weak<dyn node::Client> {
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
type SolutionQueue = Mutex<VecDeque<(work::Solution, u32)>>;

/// Helper task for `StratumClient` that implements Stratum V2 visitor which processes incoming
/// messages from remote server.
struct StratumEventHandler {
    client: Arc<StratumClient>,
    all_jobs: HashMap<u32, NewMiningJob>,
    current_prevhash_msg: Option<SetNewPrevHash>,
    /// Mining target for the next job that is to be solved
    current_target: ii_bitcoin::Target,
}

impl StratumEventHandler {
    pub fn new(client: Arc<StratumClient>, current_target: ii_bitcoin::Target) -> Self {
        Self {
            client,
            all_jobs: Default::default(),
            current_prevhash_msg: None,
            current_target,
        }
    }

    /// Convert new mining job message into StratumJob and send it down the line for solving.
    ///
    /// * `job_msg` - job message used as a base for the StratumJob
    async fn update_job(&mut self, job_msg: &NewMiningJob) {
        let job = Arc::new(StratumJob::new(
            self.client.clone(),
            job_msg,
            self.current_prevhash_msg
                .as_ref()
                .expect("TODO: no prevhash"),
            self.current_target,
        ));
        self.client.update_last_job(job.clone()).await;
        self.client.job_sender.lock().await.send(job);
    }

    fn update_target(&mut self, value: Uint256Bytes) {
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
        while let Some((solution, seq_num)) = self.client.solutions.lock().await.pop_front() {
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
        while let Some((solution, seq_num)) = self.client.solutions.lock().await.pop_front() {
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

    async fn visit_new_mining_job(&mut self, _header: &Header, job_msg: &NewMiningJob) {
        // all jobs since last `prevmsg` have to be stored in job table
        self.all_jobs.insert(job_msg.job_id, job_msg.clone());
        // TODO: close connection when maximal capacity of `all_jobs` has been reached

        // When not marked as future job, we can start mining on it right away
        if !job_msg.future_job {
            self.update_job(job_msg).await;
        }
    }

    async fn visit_set_new_prev_hash(&mut self, _header: &Header, prevhash_msg: &SetNewPrevHash) {
        self.current_prevhash_msg.replace(prevhash_msg.clone());

        // find the future job with ID referenced in prevhash_msg
        let (_, mut future_job_msg) = self
            .all_jobs
            .remove_entry(&prevhash_msg.job_id)
            .expect("TODO: requested job ID not found");

        // remove all other jobs (they are now invalid)
        self.all_jobs.retain(|_, _| true);
        // turn the job into an immediate job
        future_job_msg.future_job = false;
        // reinsert the job
        self.all_jobs
            .insert(future_job_msg.job_id, future_job_msg.clone());

        // and start immediately solving it
        self.update_job(&future_job_msg).await;
    }

    async fn visit_set_target(&mut self, _header: &Header, target_msg: &SetTarget) {
        self.update_target(target_msg.max_target);
    }

    async fn visit_submit_shares_success(
        &mut self,
        _header: &Header,
        success_msg: &SubmitSharesSuccess,
    ) {
        self.process_accepted_shares(success_msg).await;
    }

    async fn visit_submit_shares_error(&mut self, _header: &Header, error_msg: &SubmitSharesError) {
        self.process_rejected_shares(error_msg).await;
    }
}

trait FrameSink:
    Sink<<Framing as ii_wire::Framing>::Tx, Error = <Framing as ii_wire::Framing>::Error>
    + std::marker::Unpin
    + std::fmt::Debug
    + 'static
{
}

impl<T> FrameSink for T where
    T: Sink<<Framing as ii_wire::Framing>::Tx, Error = <Framing as ii_wire::Framing>::Error>
        + std::marker::Unpin
        + std::fmt::Debug
        + 'static
{
}

trait FrameStream:
    Stream<
        Item = std::result::Result<
            <Framing as ii_wire::Framing>::Tx,
            <Framing as ii_wire::Framing>::Error,
        >,
    > + std::marker::Unpin
    + 'static
{
}

impl<T> FrameStream for T where
    T: Stream<
            Item = std::result::Result<
                <Framing as ii_wire::Framing>::Tx,
                <Framing as ii_wire::Framing>::Error,
            >,
        > + std::marker::Unpin
        + 'static
{
}

struct StratumSolutionHandler<S> {
    client: Arc<StratumClient>,
    connection_tx: S,
    seq_num: u32,
}

impl<S, E> StratumSolutionHandler<S>
where
    E: Into<error::Error>,
    // TODO use S: FrameSink once the trait is adjusted to deal with payload specific error
    S: Sink<<Framing as ii_wire::Framing>::Tx, Error = E>
        + std::marker::Unpin
        + std::fmt::Debug
        + 'static,
{
    fn new(client: Arc<StratumClient>, connection_tx: S) -> Self {
        Self {
            client,
            connection_tx,
            seq_num: 0,
        }
    }

    async fn process_solution(&mut self, solution: work::Solution) -> error::Result<()> {
        let job: &StratumJob = solution.job();

        let seq_num = self.seq_num;
        self.seq_num = self.seq_num.wrapping_add(1);

        let share_msg = SubmitSharesStandard {
            channel_id: job.channel_id,
            seq_num,
            job_id: job.id,
            nonce: solution.nonce(),
            ntime: solution.time(),
            version: solution.version(),
        };
        // store solution with sequence number for future server acknowledge
        self.client
            .solutions
            .lock()
            .await
            .push_back((solution, seq_num));
        // send solutions back to the stratum server
        StratumClient::send_msg(&mut self.connection_tx, share_msg)
            .await
            .context("Cannot send submit to stratum server")?;
        // the response is handled in a separate task
        Ok(())
    }
}

struct StratumConnectionHandler {
    client: Arc<StratumClient>,
    init_target: ii_bitcoin::Target,
    status: Option<error::Result<()>>,
}

impl StratumConnectionHandler {
    pub fn new(client: Arc<StratumClient>) -> Self {
        Self {
            client,
            init_target: Default::default(),
            status: None,
        }
    }

    async fn setup_mining_connection(
        &mut self,
        connection: &mut Connection<Framing>,
    ) -> error::Result<()> {
        let setup_msg = SetupConnection {
            protocol: 0,
            max_version: 2,
            min_version: 2,
            flags: 0,
            endpoint_host: Str0_255::from_string(self.client.connection_details.host.clone()),
            endpoint_port: self.client.connection_details.port,
            device: self.client.backend_info.clone().unwrap_or_default().into(),
        };
        StratumClient::send_msg(&mut connection.tx, setup_msg)
            .await
            .context("Cannot send stratum setup mining connection")?;
        let frame = connection
            .next()
            .await
            .ok_or("The remote stratum server was disconnected prematurely")??;
        let response_msg = build_message_from_frame(frame)?;

        self.status = None;
        response_msg.accept(self).await;
        self.status.take().unwrap_or(Err(
            "Unexpected response for stratum setup mining connection".into(),
        ))
    }

    async fn open_channel(&mut self, connection: &mut Connection<Framing>) -> error::Result<()> {
        let channel_msg = OpenStandardMiningChannel {
            req_id: 10,
            user: self
                .client
                .connection_details
                .user
                .clone()
                .try_into()
                .expect("BUG: cannot convert 'OpenStandardMiningChannel::user'"),
            nominal_hashrate: 1e9,
            // Maximum bitcoin target is 0xffff << 208 (= difficulty 1 share)
            max_target: ii_bitcoin::Target::default().into(),
        };

        StratumClient::send_msg(&mut connection.tx, channel_msg)
            .await
            .context("Cannot send stratum open channel")?;
        let frame = connection
            .next()
            .await
            .ok_or("The remote stratum server was disconnected prematurely")??;
        let response_msg = build_message_from_frame(frame)?;

        self.status = None;
        response_msg.accept(self).await;
        self.status
            .take()
            .unwrap_or(Err("Unexpected response for stratum open channel".into()))
    }

    async fn connect(mut self) -> error::Result<(Connection<Framing>, ii_bitcoin::Target)> {
        // TODO the connect would always try only the first address, we should persist the list
        //  of addresses or actually we should use wire 'client' functionality for this...
        let socket_addr = self
            .client
            .connection_details
            .get_host_and_port()
            .to_socket_addrs()
            .context("Invalid server address")?
            .next()
            .ok_or("Cannot resolve any IP address")?;

        let mut connection = Connection::<Framing>::connect(&socket_addr)
            .await
            .context("Cannot connect to stratum server")?;
        self.setup_mining_connection(&mut connection)
            .await
            .context("Cannot setup stratum mining connection")?;
        self.open_channel(&mut connection)
            .await
            .context("Cannot open stratum channel")?;

        Ok((connection, self.init_target))
    }
}

#[async_trait]
impl Handler for StratumConnectionHandler {
    async fn visit_setup_connection_success(
        &mut self,
        _header: &Header,
        _success_msg: &SetupConnectionSuccess,
    ) {
        self.status = Ok(()).into();
    }

    async fn visit_setup_connection_error(
        &mut self,
        _header: &Header,
        error_msg: &SetupConnectionError,
    ) {
        self.status =
            Err(format!("Setup connection error: {}", error_msg.code.to_string()).into()).into();
    }

    async fn visit_open_standard_mining_channel_success(
        &mut self,
        _header: &Header,
        success_msg: &OpenStandardMiningChannelSuccess,
    ) {
        self.init_target = success_msg.target.into();
        self.status = Ok(()).into();
    }

    async fn visit_open_standard_mining_channel_error(
        &mut self,
        _header: &Header,
        error_msg: &OpenStandardMiningChannelError,
    ) {
        self.status =
            Err(format!("Open channel error: {}", error_msg.code.to_string()).into()).into();
    }
}

#[derive(Debug, ClientNode)]
pub struct StratumClient {
    connection_details: ConnectionDetails,
    backend_info: Option<hal::BackendInfo>,
    #[member_status]
    status: sync::StatusMonitor,
    #[member_client_stats]
    client_stats: stats::BasicClient,
    stop_sender: mpsc::Sender<()>,
    stop_receiver: Mutex<mpsc::Receiver<()>>,
    // Last job has to be weak reference to prevent circular reference (the `StratumJob` keeps
    // reference to `StratumClient`)
    last_job: Mutex<Option<Arc<StratumJob>>>,
    solutions: SolutionQueue,
    job_sender: Mutex<job::Sender>,
    solution_receiver: Mutex<job::SolutionReceiver>,
}

impl StratumClient {
    pub fn new(
        connection_details: ConnectionDetails,
        backend_info: Option<hal::BackendInfo>,
        solver: job::Solver,
        _channel: Option<()>,
    ) -> Self {
        let (stop_sender, stop_receiver) = mpsc::channel(1);
        Self {
            connection_details,
            backend_info,
            status: Default::default(),
            client_stats: Default::default(),
            stop_sender: stop_sender,
            stop_receiver: Mutex::new(stop_receiver),
            last_job: Mutex::new(None),
            solutions: Mutex::new(VecDeque::new()),
            job_sender: Mutex::new(solver.job_sender),
            solution_receiver: Mutex::new(solver.solution_receiver),
        }
    }

    async fn update_last_job(&self, job: Arc<StratumJob>) {
        self.last_job.lock().await.replace(job);
    }

    /// Send a message down a specified Tx Sink
    async fn send_msg<M, S, E>(connection_tx: &mut S, message: M) -> error::Result<()>
    where
        M: TryInto<<Framing as ii_wire::Framing>::Tx, Error = <Framing as ii_wire::Framing>::Error>,
        E: Into<error::Error>,
        // TODO use S: FrameSink once the trait is adjusted to deal with payload specific error
        S: Sink<<Framing as ii_wire::Framing>::Tx, Error = E>
            + std::marker::Unpin
            + std::fmt::Debug
            + 'static,
    {
        let frame = message.try_into()?;
        connection_tx.send(frame).await.map_err(Into::into)
    }

    async fn handle_frame(
        &self,
        frame: <Framing as ii_wire::Framing>::Rx,
        event_handler: &mut StratumEventHandler,
    ) -> error::Result<()> {
        let event_msg = build_message_from_frame(frame)?;
        event_msg.accept(event_handler).await;
        Ok(())
    }

    async fn main_loop<R, S>(
        &self,
        mut connection_rx: R,
        mut connection_tx: S,
        mut event_handler: StratumEventHandler,
        mut solution_handler: StratumSolutionHandler<
            mpsc::Sender<<Framing as ii_wire::Framing>::Tx>,
        >,
        mut solution_frame_channel_rx: mpsc::Receiver<<Framing as ii_wire::Framing>::Rx>,
    ) -> error::Result<()>
    where
        R: FrameStream,
        S: FrameSink,
    {
        let mut solution_receiver = self.solution_receiver.lock().await;

        while !self.status.is_shutting_down() {
            select! {
                frame = connection_rx.next().timeout(EVENT_TIMEOUT).fuse() => {
                    match frame {
                        Ok(Some(frame)) => self.handle_frame(frame?, &mut event_handler).await?,
                        Ok(None) | Err(_) => {
                            Err("The remote stratum server was disconnected prematurely")?;
                        }
                    }
                }
                // Forward solution frames onto the network
                frame = solution_frame_channel_rx.next().fuse() => {
                    // TODO review whether the solution RX channel may terminate
                    match frame {
                        Some(frame) => connection_tx.send(frame).await?,
                        None => {
                            Err("Solution handler terminated")?;
                        }
                    }
                }
                solution = solution_receiver.receive().fuse() => {
                    match solution {
                        Some(solution) => solution_handler.process_solution(solution).await?,
                        None => {
                            // TODO: initiate Destroying and remove error
                            Err("Standard application shutdown")?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn run_job_solver(
        self: Arc<Self>,
        connection: Connection<Framing>,
        init_target: ii_bitcoin::Target,
    ) {
        let (connection_rx, connection_tx) = connection.split();
        // Interconnect with solution handler to collect frames from it and forward them via
        // network connection
        let (solution_frame_channel_tx, solution_frame_channel_rx) =
            mpsc::channel::<<Framing as ii_wire::Framing>::Tx>(1);

        let event_handler = StratumEventHandler::new(self.clone(), init_target);
        let solution_handler = StratumSolutionHandler::new(self.clone(), solution_frame_channel_tx);
        // TODO consider changing main_loop to accept Arc<Self> and build the solution_handler
        //  along with solution handler communication channels inside of the main_loop.
        if let Err(_) = self
            .main_loop(
                connection_rx,
                connection_tx,
                event_handler,
                solution_handler,
                solution_frame_channel_rx,
            )
            .await
        {
            self.status.initiate_failing();
        }
    }

    async fn run(self: Arc<Self>) {
        match StratumConnectionHandler::new(self.clone())
            .connect()
            .timeout(CONNECTION_TIMEOUT)
            .await
        {
            Ok(Ok((connection, init_target))) => {
                if self.status.initiate_running() {
                    self.clone().run_job_solver(connection, init_target).await;
                }
            }
            Ok(Err(_)) | Err(_) => self.status.initiate_failing(),
        }
    }

    async fn main_task(self: Arc<Self>) {
        loop {
            let mut stop_receiver = self.stop_receiver.lock().await;
            select! {
                _ = self.clone().run().fuse() => {}
                _ = stop_receiver.next() => {}
            }

            // Invalidate current job to stop working on it
            self.job_sender.lock().await.invalidate();

            if self.status.can_stop() {
                // NOTE: it is not safe to add here any code!
                // The reason is that at this point the main task can be executed in parallel again
                break;
            }
            // Restarting
        }
    }
}

#[async_trait]
impl node::Client for StratumClient {
    fn start(self: Arc<Self>) {
        tokio::spawn(self.clone().main_task());
    }

    fn stop(&self) {
        if let Err(e) = self.stop_sender.clone().try_send(()) {
            assert!(
                e.is_full(),
                "BUG: Unexpected error in stop sender: {}",
                e.to_string()
            );
        }
    }

    async fn get_last_job(&self) -> Option<Arc<dyn job::Bitcoin>> {
        self.last_job
            .lock()
            .await
            .as_ref()
            .map(|job| job.clone() as Arc<dyn job::Bitcoin>)
    }
}

impl fmt::Display for StratumClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}://{}@{}",
            ClientProtocol::SCHEME_STRATUM_V2,
            self.connection_details.host,
            self.connection_details.user
        )
    }
}
