use crate::work;

use futures_locks::Mutex;

use std::sync::Arc;

/// Entry point for running the hardware backend
pub fn run(
    _work_solver: work::Solver,
    _mining_stats: Arc<Mutex<super::MiningStats>>,
    _shutdown: crate::hal::ShutdownSender,
) {
    // TODO: implement backend
}
