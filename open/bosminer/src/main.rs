#![feature(await_macro, async_await)]

use bosminer::client::stratum_v2;
use bosminer::hal;
use bosminer::runtime_config;
use bosminer::stats;
use bosminer::work;

use futures::lock::Mutex;

use std::sync::Arc;

use clap::{self, Arg};

async fn main_task(stratum_addr: String, user: String) {
    // create job and work solvers
    let (job_solver, work_solver) = work::Hub::build_solvers();
    // create shutdown channel
    let (shutdown_sender, _shutdown_receiver) = hal::Shutdown::new().split();
    // create mining stats
    let mining_stats = Arc::new(Mutex::new(hal::MiningStats::new()));

    // start HW backend for selected target
    hal::run(work_solver, mining_stats.clone(), shutdown_sender);
    // start hashrate-meter task
    ii_async_compat::spawn(stats::hashrate_meter_task_hashchain(mining_stats));
    ii_async_compat::spawn(stats::hashrate_meter_task());
    // start stratum V2 client
    await!(stratum_v2::run(job_solver, stratum_addr, user));
}

fn main() {
    let _log_guard = ii_logging::setup_for_app();

    let args = clap::App::new("bosminer")
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
            Arg::with_name("user")
                .short("u")
                .long("user")
                .value_name("NAME")
                .help("Specify user and worker name")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("disable-asic-boost")
                .long("disable-asic-boost")
                .help("Disable ASIC boost (use just one midstate)")
                .required(false),
        )
        .get_matches();

    // Unwraps should be ok as long as the flags are required
    let stratum_addr = args.value_of("pool").unwrap();
    let user = args.value_of("user").unwrap();

    // Set just 1 midstate if user requested disabling asicboost
    if args.is_present("disable-asic-boost") {
        let mut config = runtime_config::CONFIG.lock().expect("config lock failed");
        config.midstate_count = 1;
    }

    ii_async_compat::run(main_task(stratum_addr.to_string(), user.to_string()));
}
