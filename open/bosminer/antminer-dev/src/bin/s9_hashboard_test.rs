#![feature(await_macro, async_await)]

extern crate futures;
extern crate tokio;

use rminer::hal::{self, HardwareCtl};
use rminer::misc::LOGGER;
use rminer::utils;
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
    let mut dummy_job = rminer::test_utils::DummyJob::new(0);
    loop {
        job_sender.send(Arc::new(dummy_job));
        await!(Delay::new(Instant::now() + Duration::from_secs(10))).unwrap();
        dummy_job.next();
    }
}

fn main() {
    utils::run_async_main_exits(async move {
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
        tokio::spawn(hal::s9::async_hashrate_meter(mining_stats).compat_fix());

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
