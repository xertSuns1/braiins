extern crate futures;

use crate::hal;
use futures::sync::mpsc;
use futures_locks::Mutex;
use std::sync::Arc;
use tokio::await;

/// Type for solution queue
type SolutionData = ();

/// This is wrapper that asynchronously locks structure for use in
/// multiple tasks
#[derive(Clone)]
pub struct WorkHub {
    workhubdata: Arc<Mutex<WorkHubData>>,
    solution_queue_tx: mpsc::UnboundedSender<SolutionData>,
}

/// Internal structure that holds the actual work data
pub struct WorkHubData {
    midstate_start: u64,
}

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

/// This trait represents common API for work solvers to get work and
/// submit solutions
impl WorkHub {
    pub async fn get_work(&self) -> hal::MiningWork {
        await!(self.workhubdata.lock())
            .expect("locking failed")
            .get_work()
    }

    pub fn submit_solution(&self) {
        self.solution_queue_tx
            .unbounded_send(())
            .expect("solution queue send failed");
    }

    pub fn new() -> (Self, mpsc::UnboundedReceiver<SolutionData>) {
        let workhub = WorkHubData::new();
        let (tx, rx) = mpsc::unbounded();
        (
            Self {
                workhubdata: Arc::new(Mutex::new(workhub)),
                solution_queue_tx: tx,
            },
            rx,
        )
    }
}

/// * `i` - unique identifier for the generated midstate
pub fn prepare_test_work(i: u64) -> hal::MiningWork {
    hal::MiningWork {
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
