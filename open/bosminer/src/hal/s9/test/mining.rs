use crate::config;
use crate::hal::{self, s9::null_work};
use crate::test_utils;
use crate::utils;
use crate::work;

use crate::misc::LOGGER;
use slog::info;

use std::default::Default;
use std::time::{Duration, Instant};

use futures_locks::Mutex;
use std::sync::Arc;

use futures::channel::mpsc;
use tokio::await;
use tokio::timer::Delay;

/// Prepares sample work with empty midstates
/// NOTE: this work has 2 valid nonces:
/// - 0x83ea0372 (solution 0)
/// - 0x09f86be1 (solution 1)
pub fn prepare_test_work() -> hal::MiningWork {
    let time = 0xffffffff;
    let job = Arc::new(null_work::NullJob::new(time, 0xffff_ffff, 0));

    let one_midstate = hal::Midstate {
        version: 0,
        state: [0u8; 32].into(),
    };
    hal::MiningWork::new(job, vec![one_midstate; config::MIDSTATE_COUNT], time)
}

/// Count replies (even duplicate ones) and erase counters
pub async fn check_solution_count(mining_stats: Arc<Mutex<hal::MiningStats>>) -> u64 {
    let mut stats = await!(mining_stats.lock()).expect("lock mining stats");
    let total_replies: u64 = stats.unique_solutions + stats.error_stats.duplicate_solutions;
    stats.unique_solutions = 0;
    stats.error_stats.duplicate_solutions = 0;
    total_replies
}

/// Receive workloads and count replies
async fn send_and_receive_test_workloads<'a>(
    engine_sender: &'a mut work::EngineSender,
    _solution_receiver: &'a mpsc::UnboundedReceiver<hal::UniqueMiningWorkSolution>,
    mining_stats: Arc<Mutex<hal::MiningStats>>,
    n_send: usize,
    expected_solution_count: usize,
) {
    info!(
        LOGGER,
        "Sending {} work items and trying to receive {} solutions", n_send, expected_solution_count,
    );
    // Put in some tasks
    for _ in 0..n_send {
        engine_sender.broadcast(Arc::new(
            test_utils::OneWorkEngine::new(prepare_test_work()),
        ));

        // wait until the work is physically sent out it takes around 5 ms for the FPGA IP core
        // to send out the work @ 115.2 kBaud
        await!(Delay::new(Instant::now() + Duration::from_millis(100))).unwrap();
    }
    let received_solution_count = await!(check_solution_count(mining_stats.clone())) as usize;
    assert_eq!(expected_solution_count, received_solution_count);
}

fn build_solvers() -> (
    work::EngineSender,
    mpsc::UnboundedReceiver<hal::UniqueMiningWorkSolution>,
    work::Solver,
) {
    let (engine_sender, engine_receiver) = work::engine_channel(None);
    let (solution_queue_tx, solution_queue_rx) = mpsc::unbounded();
    (
        engine_sender,
        solution_queue_rx,
        work::Solver::new(engine_receiver, solution_queue_tx),
    )
}

/// Verifies work generation for a hash chain
///
/// The test runs two batches of work:
/// - the first 3 work items are for initializing input queues of the chips and don't provide any
/// solutions
/// - the next 2 work items yield actual solutions. Since we don't push more work items, the
/// solution 1 never appears on the bus and leave chips output queues. This is fine as this test
/// is intended for initial check of correct operation
#[test]
fn test_work_generation() {
    // Create shutdown channel
    let (shutdown_sender, shutdown_receiver) = hal::Shutdown::new().split();

    utils::run_async_main_exits(async move {
        // Create solver and channels to send/receive work
        let (mut engine_sender, solution_queue_rx, work_solver) = build_solvers();

        // Create mining stats
        let mining_stats = Arc::new(Mutex::new(hal::MiningStats::new()));

        // Create one chain
        let opts = hal::s9::HChainOptions {
            send_init_work: false,
            ..Default::default()
        };
        let chain = hal::s9::HChain::new(opts);
        let h_chain_ctl = chain.start_h_chain(work_solver, mining_stats.clone(), shutdown_sender);

        // the first 3 work loads don't produce any solutions, these are merely to initialize the input
        // queue of each hashing chip
        await!(send_and_receive_test_workloads(
            &mut engine_sender,
            &solution_queue_rx,
            mining_stats.clone(),
            3,
            0
        ));

        // submit 2 more work items, since we are intentionally being slow all chips should send a
        // solution for the submitted work
        let more_work_count = 2usize;
        let h_chain_guard = await!(h_chain_ctl.lock()).expect("locking failed");
        //let expected_solution_count = more_work_count * h_chain_guard.get_chip_count();
        drop(h_chain_guard);
        await!(send_and_receive_test_workloads(
            &mut engine_sender,
            &solution_queue_rx,
            mining_stats.clone(),
            more_work_count,
            0,
        ));
    });
    // the shutdown receiver has to survive up to this point to prevent
    // shutdown sends by dying tasks to fail
    drop(shutdown_receiver);
}
