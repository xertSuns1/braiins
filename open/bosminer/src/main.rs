#![feature(await_macro, async_await)]

use bosminer::client::stratum_v2;
use bosminer::hal;
use bosminer::stats;
use bosminer::work;

use ii_wire::utils::CompatFix;
use tokio::await;

use futures_locks::Mutex;

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
    tokio::spawn(stats::hashrate_meter_task(mining_stats).compat_fix());
    // start stratum V2 client
    await!(stratum_v2::run(job_solver, stratum_addr, user));
}

fn main() {
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
        .get_matches();

    // Unwraps should be ok as long as the flags are required
    let stratum_addr = args.value_of("pool").unwrap();
    let user = args.value_of("user").unwrap();

    tokio::run(main_task(stratum_addr.to_string(), user.to_string()).compat_fix());
}
