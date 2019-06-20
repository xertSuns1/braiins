use crate::hal;
use crate::workhub;

use futures::future::Future;

use std::sync::Arc;

use stratum::v2::messages::{NewMiningJob, SetNewPrevhash, SetTarget, SubmitShares};
use stratum::v2::{V2Handler, V2Protocol};
use wire::Message;

use bitcoin_hashes::{sha256d::Hash, Hash as HashTrait};
use std::collections::HashMap;

// TODO: move it to the stratum crate
const VERSION_MASK: u32 = 0x1fffe000;

#[derive(Copy, Clone)]
struct StratumJob {
    id: u32,
    channel_id: u32,
    block_height: u32,
    version: u32,
    prev_hash: Hash,
    merkle_root: Hash,
    time: u32,
    max_time: u32,
    bits: u32,
}

impl StratumJob {
    pub fn new(job_msg: &NewMiningJob, prevhash_msg: &SetNewPrevhash) -> Self {
        assert_eq!(job_msg.block_height, prevhash_msg.block_height);
        Self {
            id: job_msg.job_id,
            channel_id: job_msg.channel_id,
            block_height: job_msg.block_height,
            version: job_msg.version,
            prev_hash: Hash::from_slice(prevhash_msg.prev_hash.as_ref()).unwrap(),
            merkle_root: Hash::from_slice(job_msg.merkle_root.as_ref()).unwrap(),
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

    fn previous_hash(&self) -> &Hash {
        &self.prev_hash
    }

    fn merkle_root(&self) -> &Hash {
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
}

struct StratumEventHandler {
    job_sender: workhub::JobSender,
    new_jobs: HashMap<u32, NewMiningJob>,
}

impl StratumEventHandler {
    pub fn new(job_sender: workhub::JobSender) -> Self {
        Self {
            job_sender,
            new_jobs: Default::default(),
        }
    }
}

impl V2Handler for StratumEventHandler {
    fn visit_new_mining_job(&mut self, _msg: &Message<V2Protocol>, job_mgs: &NewMiningJob) {
        // insert new job or update old one with the same block height
        self.new_jobs.insert(job_mgs.block_height, job_mgs.clone());
        // TODO: close connection when maximal capacity of new jobs has been reached
    }

    fn visit_set_new_prevhash(
        &mut self,
        _msg: &Message<V2Protocol>,
        prevhash_msg: &SetNewPrevhash,
    ) {
        let current_block_height = prevhash_msg.block_height;
        // find a job with the same block height as provided in previous hash
        if let Some((_, job_msg)) = self.new_jobs.remove_entry(&current_block_height) {
            let job = StratumJob::new(&job_msg, prevhash_msg);
            self.job_sender.send(Arc::new(job));
        } else {
            // TODO: close connection when any job with provided block height hasn't been found
            panic!("cannot find any job for current block height");
        }

        // remove all jobs with lower block height
        self.new_jobs
            .retain(move |&block_height, _| block_height >= current_block_height);
    }

    fn visit_set_target(&mut self, _msg: &Message<V2Protocol>, target_msg: &SetTarget) {
        // TODO: use job_sender to update new target
    }
}

struct StratumSolutionHandler {
    job_solution: workhub::JobSolutionReceiver,
    seq_num: u32,
}

impl StratumSolutionHandler {
    fn new(job_solution: workhub::JobSolutionReceiver) -> Self {
        Self {
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
            ntime_offset: solution.time_offset(),
            version: solution.version(),
        };
        // TODO: send solutions back to the stratum server
    }

    async fn run(mut self) {
        while let Some(solution) = await!(self.job_solution.receive()) {
            await!(self.process_solution(solution));
        }
    }
}

pub async fn run(stratum_addr: String, job_solver: workhub::JobSolver) {
    let (job_sender, job_solution) = job_solver.split();

    // TODO: run event handler in a separate task
    let event_handler = StratumEventHandler::new(job_sender);

    await!(StratumSolutionHandler::new(job_solution).run());
}
