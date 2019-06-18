#![feature(await_macro, async_await)]

extern crate futures;
extern crate tokio;

use rminer::hal;
use rminer::hal::HardwareCtl;
use rminer::misc::LOGGER;
use rminer::workhub;

use slog::{info, trace};

use std::time::{Duration, Instant, SystemTime};

use futures::Future as OldFuture;
use futures_locks::Mutex;
use std::future::Future as NewFuture;
use std::sync::Arc;

use tokio::await;
use tokio::prelude::*;
use tokio::timer::Delay;

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

/// Convert async/await future into old style future
fn backward<I, E>(f: impl NewFuture<Output = Result<I, E>>) -> impl OldFuture<Item = I, Error = E> {
    use tokio_async_await::compat::backward;
    backward::Compat::new(f)
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
    F: NewFuture<Output = ()> + Send + 'static,
{
    use tokio::runtime::Runtime;

    let mut runtime = Runtime::new().expect("failed to start new Runtime");
    let future = backward(async move {
        await!(future);
        Result::<(), std::io::Error>::Ok(())
    });
    runtime
        .block_on(future)
        .expect("main task can't return error");
}

fn main() {
    run_async_main_exits(async move {
        // Create workhub
        let (workhub, mut rx) = workhub::WorkHub::new();

        // Create mining stats
        let mining_stats = Arc::new(Mutex::new(hal::MiningStats::new()));

        // create one chain
        let chain = hal::s9::HChain::new();
        chain.start_hw(workhub.clone(), mining_stats.clone());

        // Start hashrate-meter task
        tokio::spawn_async(async_hashrate_meter(mining_stats));

        // Receive solutions
        while let Some(_x) = await!(rx.next()) {}
    });
}
