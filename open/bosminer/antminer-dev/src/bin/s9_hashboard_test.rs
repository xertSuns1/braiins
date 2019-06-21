#![feature(await_macro, async_await)]

extern crate futures;
extern crate tokio;

use rminer::hal::{self, HardwareCtl};
use rminer::misc::LOGGER;
use rminer::workhub;

use slog::info;

use std::time::{Duration, Instant, SystemTime};

use futures_locks::Mutex;
use std::future::Future;
use std::sync::Arc;

use tokio::await;
use tokio::timer::Delay;
use wire::utils::CompatFix;

async fn dummy_job_generator(mut job_sender: workhub::JobSender) {
    let mut dummy_job = rminer::test_utils::DummyJob::new();
    loop {
        job_sender.send(Arc::new(dummy_job));
        await!(Delay::new(Instant::now() + Duration::from_secs(10))).unwrap();
        dummy_job.next();
    }
}

async fn async_hashrate_meter(mining_stats: Arc<Mutex<hal::MiningStats>>) {
    let hashing_started = SystemTime::now();
    let mut total_shares: u128 = 0;

    loop {
        await!(Delay::new(Instant::now() + Duration::from_secs(1))).unwrap();
        let mut stats = await!(mining_stats.lock()).expect("lock mining stats");
        {
            total_shares = total_shares + stats.unique_solutions as u128;
            // processing solution in the test simply means removing them
            stats.unique_solutions = 0;

            let total_hashing_time = hashing_started.elapsed().expect("time read ok");

            println!(
                "Hash rate: {} Gh/s",
                ((total_shares * (1u128 << 32)) as f32 / (total_hashing_time.as_secs() as f32))
                    * 1e-9_f32,
            );
            println!(
                "Total_shares: {}, total_time: {} s, total work generated: {}",
                total_shares,
                total_hashing_time.as_secs(),
                stats.work_generated,
            );
            println!(
                "Mismatched nonce count: {}, stale solutions: {}, duplicate solutions: {}",
                stats.mismatched_solution_nonces, stats.stale_solutions, stats.duplicate_solutions,
            );
        }
    }
}

/// Start Tokio runtime
///
/// It is much like tokio::run_async, but instead of waiting for all
/// tasks to finish, wait just for the main task.
///
/// This is a way to shutdown Tokio without diving too deep into
/// Tokio internals.
fn run_async_main_exits<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    use tokio::runtime::Runtime;

    let mut runtime = Runtime::new().expect("failed to start new Runtime");
    runtime
        .block_on(future.compat_fix())
        .expect("main task can't return error");
}

fn main() {
    run_async_main_exits(async move {
        // Create workhub
        let (work_hub, job_solver) = workhub::WorkHub::new();

        // Create mining stats
        let mining_stats = Arc::new(Mutex::new(hal::MiningStats::new()));

        // Create shutdown channel
        let (shutdown_sender, mut shutdown_receiver) = hal::Shutdown::new().split();

        // Create one chain
        let chain = hal::s9::HChain::new();
        chain.start_hw(work_hub, mining_stats.clone(), shutdown_sender);

        // Start hashrate-meter task
        tokio::spawn(async_hashrate_meter(mining_stats).compat_fix());

        let (job_sender, mut job_solution) = job_solver.split();

        // Start dummy job generator task
        tokio::spawn(dummy_job_generator(job_sender).compat_fix());

        // Receive solutions
        tokio::spawn(
            async move { while let Some(_x) = await!(job_solution.receive()) {} }.compat_fix(),
        );

        // Wait for shutdown
        let shutdown_reason = await!(shutdown_receiver.receive());
        info!(LOGGER, "SHUTDOWN: {}", shutdown_reason);
    });
}
