use crate::hal;
use crate::hal::BitcoinJob;
use bitcoin_hashes::{sha256d::Hash, Hash as HashTrait};
use byteorder::{ByteOrder, LittleEndian};
use downcast_rs::Downcast;
use futures::sync::mpsc;
use futures_locks::Mutex;
use std::sync::Arc;
use tokio::await;
use tokio::prelude::*;

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
    pub async fn generate_work(&mut self) -> hal::MiningWork {
        await!(self.0.generate())
    }

    /// Hardware-facing API
    pub fn send_solution(&self, solution: hal::UniqueMiningWorkSolution) {
        self.1.send(solution);
    }

    pub fn split(self) -> (WorkGenerator, WorkSolutionSender) {
        (self.0, self.1)
    }

    /// Construct new WorkHub and associated queue to send work through
    /// This is runner/orchestrator/pump-facing function
    pub fn new() -> (Self, JobSolver) {
        let (job_queue_tx, job_queue_rx) = mpsc::unbounded();
        let (solution_queue_tx, solution_queue_rx) = mpsc::unbounded();
        (
            Self(
                WorkGenerator(job_queue_rx.peekable()),
                WorkSolutionSender(solution_queue_tx),
            ),
            JobSolver(
                JobSender(job_queue_tx),
                JobSolutionReceiver(solution_queue_rx),
            ),
        )
    }
}

pub struct WorkGenerator(stream::Peekable<mpsc::UnboundedReceiver<Arc<dyn hal::BitcoinJob>>>);

impl WorkGenerator {
    pub async fn generate(&mut self) -> hal::MiningWork {
        await!(self.0.next());
        prepare_test_work(0)
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
    pub fn send_job(&self, job: Arc<dyn hal::BitcoinJob>) {
        self.0.send(job)
    }

    pub async fn receive_solution(&mut self) -> Option<hal::UniqueMiningWorkSolution> {
        await!(self.1.receive())
    }

    pub fn split(self) -> (JobSender, JobSolutionReceiver) {
        (self.0, self.1)
    }
}

#[derive(Clone)]
pub struct JobSender(mpsc::UnboundedSender<Arc<dyn hal::BitcoinJob>>);

impl JobSender {
    pub fn send(&self, job: Arc<dyn hal::BitcoinJob>) {
        self.0.unbounded_send(job).expect("job queue send failed");
    }
}

pub struct JobSolutionReceiver(mpsc::UnboundedReceiver<hal::UniqueMiningWorkSolution>);

impl JobSolutionReceiver {
    pub async fn receive(&mut self) -> Option<hal::UniqueMiningWorkSolution> {
        if let Some(Ok(solution)) = await!(self.0.next()) {
            Some(solution)
        } else {
            None
        }
    }
}

struct DummyJob(Hash);

impl DummyJob {
    pub fn new() -> Self {
        DummyJob(Hash::from_slice(&[0xffu8; 32]).unwrap())
    }
}

impl hal::BitcoinJob for DummyJob {
    fn version(&self) -> u32 {
        0
    }

    fn version_mask(&self) -> u32 {
        0
    }

    fn previous_hash(&self) -> &Hash {
        &self.0
    }

    fn merkle_root(&self) -> &Hash {
        &self.0
    }

    fn time(&self) -> u32 {
        0xffff_ffff
    }

    fn bits(&self) -> u32 {
        0xffff_ffff
    }
}

/// * `i` - unique identifier for the generated midstate
pub fn prepare_test_work(i: u64) -> hal::MiningWork {
    let job = Arc::new(DummyJob::new());
    let time = job.time();

    let mut mid = hal::Midstate {
        version: 0,
        state: [0u8; 32],
    };
    LittleEndian::write_u64(&mut mid.state, i);

    hal::MiningWork {
        job,
        midstates: vec![mid],
        ntime: time,
    }
}
