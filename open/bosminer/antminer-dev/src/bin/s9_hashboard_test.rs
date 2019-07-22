#![feature(await_macro, async_await)]

use rminer::hal;
use rminer::misc::LOGGER;
use rminer::stats;
use rminer::utils;
use rminer::work;

use slog::info;

use std::time::{Duration, Instant};

use futures_locks::Mutex;
use std::sync::Arc;

use tokio::await;
use tokio::timer::Delay;
use wire::utils::CompatFix;

async fn dummy_job_generator(mut job_sender: work::JobSender) {
    let mut dummy_job = hal::s9::null_work::NullJob::new(0);
    loop {
        job_sender.send(Arc::new(dummy_job));
        await!(Delay::new(Instant::now() + Duration::from_secs(10))).unwrap();
        dummy_job.next();
    }
}

fn main() {
    utils::run_async_main_exits(async move {
        // Create workhub
        let (job_solver, work_solver) = work::Hub::build_solvers();
        let (job_sender, mut job_solution) = job_solver.split();
        // Create mining stats
        let mining_stats = Arc::new(Mutex::new(hal::MiningStats::new()));
        // Create shutdown channel
        let (shutdown_sender, mut shutdown_receiver) = hal::Shutdown::new().split();

        hal::run(work_solver, mining_stats.clone(), shutdown_sender);
        // Start hashrate-meter task
        tokio::spawn(stats::hashrate_meter_task(mining_stats).compat_fix());
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
