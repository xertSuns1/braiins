#![feature(await_macro, async_await)]

extern crate futures;
extern crate tokio;

use rminer::client::stratum_v2;
use rminer::hal::{self, HardwareCtl};
use rminer::workhub;

use std::time::{Duration, Instant, SystemTime};

use futures_locks::Mutex;
use std::sync::Arc;

use tokio::await;
use tokio::timer::Delay;
use wire::utils::CompatFix;

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

async fn main_task() {
    let (work_hub, job_solver) = workhub::WorkHub::new();

    // Create mining stats
    let mining_stats = Arc::new(Mutex::new(hal::MiningStats::new()));

    // Create shutdown channel
    let (shutdown_sender, _shutdown_receiver) = hal::Shutdown::new().split();

    // Create one chain
    let chain = hal::s9::HChain::new();
    chain.start_hw(work_hub, mining_stats.clone(), shutdown_sender);

    // Start hashrate-meter task
    tokio::spawn(async_hashrate_meter(mining_stats).compat_fix());

    // Start stratum V2 client
    await!(stratum_v2::run(
        job_solver,
        "10.33.10.144:3333".to_string(),
        "braiins.worker0".to_string()
    ));
}

fn main() {
    tokio::run(main_task().compat_fix());
}
