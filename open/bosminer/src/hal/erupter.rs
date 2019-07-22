pub mod device;
pub mod error;
pub mod icarus;

use crate::utils::compat_block_on;
use crate::work;

use crate::misc::LOGGER;
use slog::{error, info};

use tokio_threadpool::blocking;

// use old futures which is compatible with current tokio
use futures_01::future::poll_fn;
use futures_locks::Mutex;

use std::sync::Arc;

fn main_task(
    work_solver: work::Solver,
    _mining_stats: Arc<Mutex<super::MiningStats>>,
    _shutdown: crate::hal::ShutdownSender,
) -> crate::error::Result<()> {
    let (mut generator, _solution_sender) = work_solver.split();

    info!(LOGGER, "Erupter: waiting for work...");
    let work = compat_block_on(generator.generate()).unwrap();
    info!(LOGGER, "Erupter: {:?}", work.job);
    Ok(())
}

/// Entry point for running the hardware backend
pub fn run(
    work_solver: work::Solver,
    mining_stats: Arc<Mutex<super::MiningStats>>,
    shutdown: crate::hal::ShutdownSender,
) {
    // wrap the work solver to Option to overcome FnOnce closure inside FnMut
    let mut args = Some((work_solver, mining_stats, shutdown));

    // spawn future in blocking context which guarantees that the task is run in separate thread
    tokio::spawn(
        // Because `blocking` returns `Poll`, it is intended to be used from the context of
        // a `Future` implementation. Since we don't have a complicated requirement, we can use
        // `poll_fn` in this case.
        poll_fn(move || {
            blocking(|| {
                let (work_solver, mining_stats, shutdown) = args
                    .take()
                    .expect("`tokio_threadpool::blocking` called FnOnce more than once");
                if let Err(e) = main_task(work_solver, mining_stats, shutdown) {
                    error!(LOGGER, "{}", e);
                }
            })
            .map_err(|_| panic!("the threadpool shut down"))
        }),
    );
}
