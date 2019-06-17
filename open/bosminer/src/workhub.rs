extern crate futures;

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

/// Internal structure that holds the actual work data
pub struct WorkHubData {
    midstate_start: u64,
}

impl WorkHubData {
    pub fn get_work(&mut self) -> Option<hal::MiningWork> {
        let work = prepare_test_work(self.midstate_start);
        // the midstate identifier may wrap around (considering its size, effectively never...)
        self.midstate_start = self.midstate_start.wrapping_add(1);
        Some(work)
    }

    pub fn new() -> Self {
        Self { midstate_start: 0 }
    }
}

/// This is wrapper that asynchronously locks structure for use in
/// multiple tasks
#[derive(Clone)]
pub struct WorkHub {
    workhub_data: Arc<Mutex<WorkHubData>>,
    solution_queue_tx: mpsc::UnboundedSender<hal::UniqueMiningWorkSolution>,
}

/// This trait represents common API for work solvers to get work and
/// submit solutions
impl WorkHub {
    /// Hardware-facing API
    pub async fn get_work(&self) -> Option<hal::MiningWork> {
        await!(self.workhub_data.lock())
            .expect("locking failed")
            .get_work()
    }

    /// Hardware-facing API
    pub fn submit_solution(&self, solution: hal::UniqueMiningWorkSolution) {
        self.solution_queue_tx
            .unbounded_send(solution)
            .expect("solution queue send failed");
    }

    /// Construct new WorkHub and associated queue to send work through
    /// This is runner/orchestrator/pump-facing function
    pub fn new() -> (Self, JobSolver) {
        let (tx, rx) = mpsc::unbounded();
        let workhub_data = Arc::new(Mutex::new(WorkHubData::new()));
        (
            Self {
                workhub_data: workhub_data.clone(),
                solution_queue_tx: tx,
            },
            JobSolver {
                workhub_data,
                solution_queue_rx: rx,
            },
        )
    }
}

pub struct JobSolver {
    workhub_data: Arc<Mutex<WorkHubData>>,
    solution_queue_rx: mpsc::UnboundedReceiver<hal::UniqueMiningWorkSolution>,
}

impl JobSolver {
    pub fn send_job(&self, job: Arc<dyn hal::BitcoinJob>) {
        // TODO:
    }

    pub async fn receive_solution(&mut self) -> Option<hal::UniqueMiningWorkSolution> {
        if let Some(Ok(solution)) = await!(self.solution_queue_rx.next()) {
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
