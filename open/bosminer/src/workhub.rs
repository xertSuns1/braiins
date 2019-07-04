use crate::btc;
use crate::hal::{self, BitcoinJob};

use crate::misc::LOGGER;
use slog::{info, trace};

use futures::channel::mpsc;
use futures::stream::StreamExt;
use tokio::sync::watch;
use tokio_async_await::stream::StreamExt as StreamExtForWatchBroadcast;

use std::sync::{Arc, Mutex as StdMutex, RwLock};

use bitcoin_hashes::{sha256, Hash, HashEngine};
use byteorder::{ByteOrder, LittleEndian};

const DIFFICULTY_1_TARGET_BYTES: [u8; btc::SHA256_DIGEST_SIZE] = [
    0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// A registry of solutions
#[allow(dead_code)]
struct SolutionRegistry {
    /// Unique solutions
    solutions: std::vec::Vec<hal::UniqueMiningWorkSolution>,
}

#[allow(dead_code)]
impl SolutionRegistry {
    fn new() -> Self {
        Self {
            solutions: std::vec::Vec::new(),
        }
    }
}

pub struct WorkHub(WorkGenerator, WorkSolutionSender);

/// This trait represents common API for work solvers to get work and
/// submit solutions
impl WorkHub {
    /// Hardware-facing API
    pub async fn generate_work(&mut self) -> Option<hal::MiningWork> {
        await!(self.0.generate())
    }

    /// Hardware-facing API
    pub fn send_solution(&self, solution: hal::UniqueMiningWorkSolution) {
        self.1.send(solution);
    }

    /// For debugging purposes
    pub fn set_inject_work_queue(&mut self, q: mpsc::UnboundedReceiver<hal::MiningWork>) {
        self.0.inject_work_queue = Some(q);
    }

    pub fn split(self) -> (WorkGenerator, WorkSolutionSender) {
        (self.0, self.1)
    }

    /// Construct new WorkHub and associated queue to send work through
    /// This is runner/orchestrator/pump-facing function
    pub fn new() -> (Self, JobSolver) {
        let current_target = Arc::new(RwLock::new(uint::U256::from_big_endian(
            &DIFFICULTY_1_TARGET_BYTES,
        )));
        let (job_broadcast_tx, job_broadcast_rx) = watch::channel(None);
        let (solution_queue_tx, solution_queue_rx) = mpsc::unbounded();
        (
            Self(
                WorkGenerator::new(job_broadcast_rx),
                WorkSolutionSender(solution_queue_tx),
            ),
            JobSolver(
                JobSender::new(job_broadcast_tx, current_target.clone()),
                JobSolutionReceiver::new(solution_queue_rx, current_target),
            ),
        )
    }
}

pub struct NewJobEvent;

type WrappedJob = Option<Arc<dyn BitcoinJob>>;
type JobChannelReceiver = watch::Receiver<WrappedJob>;
type JobChannelSender = watch::Sender<WrappedJob>;

struct JobQueue {
    job_broadcast_rx: JobChannelReceiver,
    current_job: Option<Arc<dyn BitcoinJob>>,
    finished: bool,
}

impl JobQueue {
    pub fn new(job_broadcast_rx: JobChannelReceiver) -> Self {
        Self {
            job_broadcast_rx,
            current_job: None,
            finished: true,
        }
    }

    /// Returns current job from which the new work is generated
    /// When the current job has been replaced with a new one
    /// then it is indicated in the second return value
    pub async fn determine_current_job(&mut self) -> (Arc<dyn BitcoinJob>, bool) {
        // look at latest broadcasted job
        match self.job_broadcast_rx.get_ref().as_ref() {
            // no job has been broadcasted yet, wait
            None => (),
            // check if we are working on anything
            Some(latest_job) => match self.current_job {
                // we aren't, so work on the latest job
                None => return (latest_job.clone(), true),
                Some(ref current_job) => {
                    // is our current job different from latest?
                    if !Arc::ptr_eq(current_job, latest_job) {
                        // something new has been broadcasted, work on that
                        return (latest_job.clone(), true);
                    }
                    // if we haven't finished it, continue working on it
                    if !self.finished {
                        return (current_job.clone(), false);
                    }
                    // otherwise just wait for more work
                }
            },
        }
        // loop until we receive a job
        loop {
            let new_job = await!(self.job_broadcast_rx.next())
                .expect("job reception failed")
                .expect("job stream ended");
            if let Some(new_job) = new_job {
                return (new_job, true);
            }
        }
    }

    pub async fn get_job(&mut self) -> (Arc<dyn BitcoinJob>, bool) {
        let (job, is_new) = await!(self.determine_current_job());
        if is_new {
            self.current_job = Some(job.clone())
        }
        self.finished = false;
        (job, is_new)
    }

    /// Clears the current job when the whole address space is exhausted
    /// After this method has been called, the get_job starts blocking until
    /// the new job is delivered
    pub fn finish_current_job(&mut self) {
        info!(LOGGER, "--- finishing current job ---");
        self.finished = true;
    }
}

pub struct WorkGenerator {
    pub inject_work_queue: Option<mpsc::UnboundedReceiver<hal::MiningWork>>,
    job_queue: JobQueue,
    midstates: usize,
    next_version: u16,
    base_version: u32,
}

impl WorkGenerator {
    pub fn new(job_channel: JobChannelReceiver) -> Self {
        Self {
            inject_work_queue: None,
            job_queue: JobQueue::new(job_channel),
            midstates: 1,
            next_version: 0,
            base_version: 0,
        }
    }

    /// Roll new versions for Bitcoin header for all midstates
    /// It finishes (clears) the current job if it determines then no new version
    /// cannot be generated
    fn next_versions(&mut self, job: &Arc<dyn BitcoinJob>, new_job: bool) -> Vec<u32> {
        const MASK: u32 = 0x1fffe000;
        const SHIFT: u32 = 13;

        let version_start;
        if new_job {
            version_start = 0;
            self.next_version = self.midstates as u16;
            self.base_version = job.version() & !MASK;
        } else {
            version_start = self.next_version;
            if let Some(next_version) = self.next_version.checked_add(self.midstates as u16) {
                self.next_version = next_version;
            } else {
                self.job_queue.finish_current_job();
                self.next_version = 0;
            }
        };

        let mut versions = Vec::with_capacity(self.midstates);
        for version in version_start..self.next_version {
            versions.push(self.base_version | ((version as u32) << SHIFT));
        }
        versions
    }

    fn get_work(&mut self, job: Arc<dyn BitcoinJob>, versions: Vec<u32>) -> hal::MiningWork {
        let time = job.time();
        let mut midstates = Vec::with_capacity(versions.len());

        let mut engine = sha256::Hash::engine();
        let buffer = &mut [0u8; 64];

        buffer[4..36].copy_from_slice(&job.previous_hash().into_inner());
        buffer[36..64].copy_from_slice(&job.merkle_root().into_inner()[..32 - 4]);

        for version in versions {
            LittleEndian::write_u32(&mut buffer[0..4], version);
            engine.input(buffer);
            midstates.push(hal::Midstate {
                version,
                state: engine.midstate(),
            })
        }

        hal::MiningWork {
            job,
            midstates,
            ntime: time,
        }
    }

    /// Returns new work generated from the current job
    pub async fn generate(&mut self) -> Option<hal::MiningWork> {
        // in case work injection queue is present, get work from there
        // instead of trying to build work from job
        if let Some(ref mut work_rx) = self.inject_work_queue {
            return await!(work_rx.next());
        }
        let (job, new_job) = await!(self.job_queue.get_job());

        let versions = self.next_versions(&job, new_job);
        Some(self.get_work(job, versions))
    }
}

#[derive(Clone)]
pub struct WorkSolutionSender(mpsc::UnboundedSender<hal::UniqueMiningWorkSolution>);

impl WorkSolutionSender {
    pub fn send(&self, solution: hal::UniqueMiningWorkSolution) {
        self.0
            .unbounded_send(solution)
            .expect("solution queue send failed");
    }
}

pub struct JobSolver(JobSender, JobSolutionReceiver);

impl JobSolver {
    pub fn send_job(&mut self, job: Arc<dyn hal::BitcoinJob>) {
        self.0.send(job)
    }

    pub async fn receive_solution(&mut self) -> Option<hal::UniqueMiningWorkSolution> {
        await!(self.1.receive())
    }

    pub fn split(self) -> (JobSender, JobSolutionReceiver) {
        (self.0, self.1)
    }
}

pub struct JobSender {
    job_broadcast_tx: JobChannelSender,
    current_target: Arc<RwLock<uint::U256>>,
}

impl JobSender {
    pub fn new(
        job_broadcast_tx: JobChannelSender,
        current_target: Arc<RwLock<uint::U256>>,
    ) -> Self {
        Self {
            job_broadcast_tx,
            current_target,
        }
    }

    pub fn change_target(&self, target: uint::U256) {
        *self
            .current_target
            .write()
            .expect("cannot write to shared current target") = target;
    }

    pub fn send(&mut self, job: Arc<dyn hal::BitcoinJob>) {
        info!(LOGGER, "--- broadcasting new job ---");
        if self.job_broadcast_tx.broadcast(Some(job)).is_err() {
            panic!("job broadcast failed");
        }
    }
}

pub struct JobSolutionReceiver {
    solution_channel: mpsc::UnboundedReceiver<hal::UniqueMiningWorkSolution>,
    current_target: Arc<RwLock<uint::U256>>,
}

impl JobSolutionReceiver {
    pub fn new(
        solution_channel: mpsc::UnboundedReceiver<hal::UniqueMiningWorkSolution>,
        current_target: Arc<RwLock<uint::U256>>,
    ) -> Self {
        Self {
            solution_channel,
            current_target,
        }
    }

    fn trace_share(solution: &hal::UniqueMiningWorkSolution, target: &uint::U256) {
        // TODO: create specialized structure 'Target' and rewrite it
        let mut xtarget = [0u8; 32];
        target.to_big_endian(&mut xtarget[..]);

        trace!(
            LOGGER,
            "nonce={:08x} bytes={}",
            solution.nonce(),
            hex::encode(&solution.get_block_header().into_bytes()[..])
        );
        trace!(LOGGER, "  hash={:x}", solution.hash());
        trace!(LOGGER, "target={}", hex::encode(xtarget));
    }

    pub async fn receive(&mut self) -> Option<hal::UniqueMiningWorkSolution> {
        while let Some(solution) = await!(self.solution_channel.next()) {
            let current_target = &*self
                .current_target
                .read()
                .expect("cannot read from shared current target");
            if solution.is_valid(current_target) {
                info!(LOGGER, "----- SHARE BELLOW TARGET -----");
                Self::trace_share(&solution, &current_target);
                return Some(solution);
            }
        }
        None
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::test_utils;

    #[test]
    fn test_block_midstate() {
        for block in test_utils::TEST_BLOCKS.iter() {
            let version = block.version();
            let (job_broadcast_tx, job_broadcast_rx) = watch::channel(None);
            let job_queue = JobQueue {
                job_broadcast_rx,
                current_job: None,
                finished: false,
            };
            let mut generator = WorkGenerator {
                inject_work_queue: None,
                job_queue,
                midstates: 1,
                next_version: 0,
                base_version: version,
            };

            let work = generator.get_work(Arc::new(*block), vec![version]);

            assert_eq!(block.midstate, work.midstates[0].state);
        }
    }
}
