extern crate futures;

use crate::hal;
use bitcoin_hashes::{sha256d::Hash, Hash as HashTrait};
use futures::sync::mpsc;
use futures_locks::Mutex;
use std::any::Any;
use std::sync::Arc;
use tokio::await;

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
    pub fn get_work(&mut self) -> hal::MiningWork {
        let work = prepare_test_work(self.midstate_start);
        // the midstate identifier may wrap around (considering its size, effectively never...)
        self.midstate_start = self.midstate_start.wrapping_add(1);
        work
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
    pub async fn get_work(&self) -> hal::MiningWork {
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
    pub fn new() -> (Self, mpsc::UnboundedReceiver<hal::UniqueMiningWorkSolution>) {
        let (tx, rx) = mpsc::unbounded();
        (
            Self {
                workhub_data: Arc::new(Mutex::new(WorkHubData::new())),
                solution_queue_tx: tx,
            },
            rx,
        )
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
    hal::MiningWork {
        job: Arc::new(DummyJob::new()),
        version: 0,
        extranonce_2: 0,
        midstates: vec![uint::U256([i, 0, 0, 0])],
        merkel_root_lsw: 0xffff_ffff,
        nbits: 0xffff_ffff,
        ntime: 0xffff_ffff,
        //            version: 0,
        //            extranonce_2: 0,
        //            midstates: vec![uint::U256([v, 2, 3, 4])],
        //            merkel_root_lsw: 0xdeadbeef,
        //            nbits: 0x1a44b9f2,
        //            ntime: 0x4dd7f5c7,
    }
}
