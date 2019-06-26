#![feature(await_macro, async_await)]

use clap::{self, Arg};

use rminer::client::stratum_v2;
use rminer::hal::{self, HardwareCtl};
use rminer::workhub;

use std::time::{Duration, Instant, SystemTime};

use futures_locks::Mutex;
use std::sync::Arc;

use tokio::await;
use tokio::timer::Delay;
use wire::utils::CompatFix;

async fn main_task(stratum_addr: String, user: String) {
    let (work_hub, job_solver) = workhub::WorkHub::new();

    // Create mining stats
    let mining_stats = Arc::new(Mutex::new(hal::MiningStats::new()));

    // Create shutdown channel
    let (shutdown_sender, _shutdown_receiver) = hal::Shutdown::new().split();

    // Create one chain
    let chain = hal::s9::HChain::new();
    chain.start_hw(work_hub, mining_stats.clone(), shutdown_sender);

    // Start hashrate-meter task
    tokio::spawn(hal::s9::async_hashrate_meter(mining_stats).compat_fix());

    // Start stratum V2 client
    await!(stratum_v2::run(job_solver, stratum_addr, user));
}

fn main() {
    let args = clap::App::new("s9-stratum-test")
        .arg(
            Arg::with_name("pool")
                .short("p")
                .long("pool")
                .value_name("URL")
                .help("Address the stratum V2 server")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("test-threads")
                .short("t")
                .long("test-threads")
                .value_name("test threads")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("user")
                .short("u")
                .long("user")
                .value_name("NAME")
                .help("Specify user and worker name")
                .required(true)
                .takes_value(true),
        )
        .get_matches();

    // Unwraps should be ok as long as the flags are required
    let stratum_addr = args.value_of("pool").unwrap();
    let user = args.value_of("user").unwrap();

    tokio::run(main_task(stratum_addr.to_string(), user.to_string()).compat_fix());
}
