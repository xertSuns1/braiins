use ii_logging::macros::*;

use ii_bitcoin::HashTrait;

use crate::hal;
use crate::work;

use tokio::prelude::*;
use tokio::r#await;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use ii_stratum::v2::framing::codec::Framing;
use ii_stratum::v2::messages::{
    NewMiningJob, OpenChannel, OpenChannelError, OpenChannelSuccess, SetNewPrevHash, SetTarget,
    SetupMiningConnection, SetupMiningConnectionError, SetupMiningConnectionSuccess, SubmitShares,
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
    id: u32,
    channel_id: u32,
    block_height: u32,
    current_block_height: Arc<AtomicU32>,
    version: u32,
    prev_hash: ii_bitcoin::DHash,
    merkle_root: ii_bitcoin::DHash,
    time: u32,
    max_time: u32,
    bits: u32,
}

impl StratumJob {
    pub fn new(
        job_msg: &NewMiningJob,
        prevhash_msg: &SetNewPrevHash,
        current_block_height: Arc<AtomicU32>,
    ) -> Self {
        assert_eq!(job_msg.block_height, prevhash_msg.block_height);
        Self {
            id: job_msg.job_id,
            channel_id: job_msg.channel_id,
            block_height: job_msg.block_height,
            current_block_height,
            version: job_msg.version,
            prev_hash: ii_bitcoin::DHash::from_slice(prevhash_msg.prev_hash.as_ref()).unwrap(),
            merkle_root: ii_bitcoin::DHash::from_slice(job_msg.merkle_root.as_ref()).unwrap(),
            time: prevhash_msg.min_ntime,
            max_time: prevhash_msg.min_ntime + prevhash_msg.max_ntime_offset as u32,
            bits: prevhash_msg.nbits,
        }
    }
}

impl hal::BitcoinJob for StratumJob {
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

    fn is_valid(&self) -> bool {
        self.block_height >= self.current_block_height.load(Ordering::Relaxed)
    }
}

struct StratumEventHandler {
    status: Result<(), ()>,
    job_sender: work::JobSender,
    all_jobs: HashMap<u32, NewMiningJob>,
    current_block_height: Arc<AtomicU32>,
    current_prevhash_msg: Option<SetNewPrevHash>,
}

impl StratumEventHandler {
    pub fn new(job_sender: work::JobSender) -> Self {
        Self {
            status: Err(()),
            job_sender,
            all_jobs: Default::default(),
            current_block_height: Arc::new(AtomicU32::new(0)),
            current_prevhash_msg: None,
        }
    }
    pub fn update_job(&mut self, job_msg: &NewMiningJob) {
        let job = StratumJob::new(
            job_msg,
            self.current_prevhash_msg.as_ref().expect("no prevhash"),
            self.current_block_height.clone(),
        );
        self.job_sender.send(Arc::new(job));
    }

    pub fn update_target(&mut self, value: Uint256Bytes) {
        let new_target: ii_bitcoin::Target = value.into();
        trace!("changing target to {:?}", new_target);
        self.job_sender.change_target(new_target);
    }
}

impl Handler for StratumEventHandler {
    // The rules for prevhash/mining job pairing are (currently) as follows:
    //  - when mining job comes
    //      - store it (by id)
    //      - start mining it if it's at the same blockheight
    //  - when prevhash message comes
    //      - replace it
    //      - start mining the job it references (by job id)
    //      - flush all other jobs

    fn visit_new_mining_job(&mut self, _msg: &Message<Protocol>, job_msg: &NewMiningJob) {
        // all jobs since last `prevmsg` have to be stored in job table
        // TODO: use job ID instead of block_height
        self.all_jobs.insert(job_msg.block_height, job_msg.clone());
        // TODO: close connection when maximal capacity of `all_jobs` has been reached

        // already solving this blockheight?
        if job_msg.block_height == self.current_block_height.load(Ordering::Relaxed)
            && self.current_prevhash_msg.is_some()
        {
            // ... yes, switch jobs straight away
            // also: there's an invariant: current_block_height == current_prevhash_msg.block_height
            self.update_job(job_msg);
        }
    }

    fn visit_set_new_prev_hash(&mut self, _msg: &Message<Protocol>, prevhash_msg: &SetNewPrevHash) {
        let current_block_height = prevhash_msg.block_height;
        // immediately update current block height which is propagated to currently solved jobs
        self.current_block_height
            .store(current_block_height, Ordering::Relaxed);
        self.current_prevhash_msg.replace(prevhash_msg.clone());

        // find a job with ID referenced in prevhash_msg
        // TODO: really use the job id, not just block_height
        let (_, job_msg) = self
            .all_jobs
            .remove_entry(&current_block_height)
            .expect("requested jobid not found");

        // remove all other jobs (they are now invalid)
        self.all_jobs.retain(|_, _| true);

        // reinsert the job
        self.all_jobs.insert(job_msg.block_height, job_msg.clone());

        // and start immediately solving it
        self.update_job(&job_msg);
    }

    fn visit_set_target(&mut self, _msg: &Message<Protocol>, target_msg: &SetTarget) {
        self.update_target(target_msg.max_target);
    }

    fn visit_setup_mining_connection_success(
        &mut self,
        _msg: &Message<Protocol>,
        _success_msg: &SetupMiningConnectionSuccess,
    ) {
        self.status = Ok(());
    }

    fn visit_setup_mining_connection_error(
        &mut self,
        _msg: &Message<Protocol>,
        _error_msg: &SetupMiningConnectionError,
    ) {
        self.status = Err(());
    }

    fn visit_open_channel_success(
        &mut self,
        _msg: &Message<Protocol>,
        success_msg: &OpenChannelSuccess,
    ) {
        self.update_target(success_msg.init_target);
        self.status = Ok(());
    }

    fn visit_open_channel_error(
        &mut self,
        _msg: &Message<Protocol>,
        _error_msg: &OpenChannelError,
    ) {
        self.status = Err(());
    }
}

struct StratumSolutionHandler {
    connection_tx: ConnectionTx<Framing>,
    job_solution: work::JobSolutionReceiver,
    seq_num: u32,
}

impl StratumSolutionHandler {
    fn new(connection_tx: ConnectionTx<Framing>, job_solution: work::JobSolutionReceiver) -> Self {
        Self {
            connection_tx,
            job_solution,
            seq_num: 0,
        }
    }

    async fn process_solution(&mut self, solution: hal::UniqueMiningWorkSolution) {
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
        await!(ConnectionTx::send(&mut self.connection_tx, share_msg))
            .expect("Cannot send submit to stratum server");
        // the response is handled in a separate task
    }

    async fn run(mut self) {
        while let Some(solution) = await!(self.job_solution.receive()) {
            await!(self.process_solution(solution));
        }
    }
}

async fn setup_mining_connection<'a>(
    connection: &'a mut Connection<Framing>,
    event_handler: &'a mut StratumEventHandler,
    stratum_addr: String,
) -> Result<(), ()> {
    let setup_msg = SetupMiningConnection {
        protocol_version: 0,
        connection_url: stratum_addr,
        /// header only mining
        required_extranonce_size: 0,
    };
    await!(connection.send(setup_msg)).expect("Cannot send stratum setup mining connection");
    let response_msg = await!(connection.next())
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
    let channel_msg = OpenChannel {
        req_id: 10,
        user,
        extended: false,
        device: DeviceInfo {
            vendor: "Braiins".to_string(),
            hw_rev: "1".to_string(),
            fw_ver: "Braiins OS 2019-06-05".to_string(),
            dev_id: "xyz".to_string(),
        },
        nominal_hashrate: 1e9,
        // Maximum bitcoin target is 0xffff << 208 (= difficulty 1 share)
        max_target_nbits: 0x1d00ffff,
        aggregated_device_count: 1,
    };
    await!(connection.send(channel_msg)).expect("Cannot send stratum open channel");
    let response_msg = await!(connection.next())
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
    fn visit_setup_mining_connection(
        &mut self,
        _msg: &ii_wire::Message<Protocol>,
        payload: &SetupMiningConnection,
    ) {
        self.0 = Some(format!("{:?}", payload));
    }

    fn visit_setup_mining_connection_success(
        &mut self,
        _msg: &ii_wire::Message<Protocol>,
        payload: &SetupMiningConnectionSuccess,
    ) {
        self.0 = Some(format!("{:?}", payload));
    }

    fn visit_open_channel(&mut self, _msg: &ii_wire::Message<Protocol>, payload: &OpenChannel) {
        self.0 = Some(format!("{:?}", payload));
    }

    fn visit_open_channel_success(
        &mut self,
        _msg: &ii_wire::Message<Protocol>,
        payload: &OpenChannelSuccess,
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
    while let Some(msg) = await!(connection_rx.next()) {
        let msg = msg.unwrap();
        trace!("handling message {}", StringifyV2::print(&msg));
        msg.accept(&mut event_handler);
    }
}

pub async fn run(job_solver: work::JobSolver, stratum_addr: String, user: String) {
    let socket_addr = stratum_addr.parse().expect("Invalid server address");
    let (job_sender, job_solution) = job_solver.split();
    let mut event_handler = StratumEventHandler::new(job_sender);

    let mut connection = await!(Connection::<Framing>::connect(&socket_addr))
        .expect("Cannot connect to stratum server");

    await!(setup_mining_connection(
        &mut connection,
        &mut event_handler,
        stratum_addr
    ))
    .expect("Cannot setup stratum mining connection");
    await!(open_channel(&mut connection, &mut event_handler, user))
        .expect("Cannot open stratum channel");

    let (connection_rx, connection_tx) = connection.split();

    // run event handler in a separate task
    ii_async_compat::spawn(event_handler_task(connection_rx, event_handler));

    await!(StratumSolutionHandler::new(connection_tx, job_solution).run());
}
