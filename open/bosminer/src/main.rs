// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

#![feature(await_macro, async_await)]

use bosminer::client::stratum_v2;
use bosminer::hal;
use bosminer::runtime_config;
use bosminer::stats;
use bosminer::work;

use futures::lock::Mutex;

use std::sync::Arc;

use clap::{self, Arg};

/// Starts statistics tasks specific for S9
/// TODO: This function is to be removed once we replace the stats module with a more robust
/// solution
#[cfg(feature = "antminer_s9")]
fn start_mining_stats_task(mining_stats: Arc<Mutex<stats::Mining>>) {
    ii_async_compat::spawn(stats::hashrate_meter_task_hashchain(mining_stats));
    ii_async_compat::spawn(stats::hashrate_meter_task());
}

/// Starts statistics tasks specific for block erupter
/// TODO: to be removed, see above
#[cfg(feature = "erupter")]
fn start_mining_stats_task(_mining_stats: Arc<Mutex<stats::Mining>>) {
    ii_async_compat::spawn(stats::hashrate_meter_task());
}

async fn main_task(stratum_addr: String, user: String) {
    // create job and work solvers
    let (job_solver, work_solver) = work::Hub::build_solvers();
    // create shutdown channel
    let (shutdown_sender, _shutdown_receiver) = hal::Shutdown::new().split();
    // create mining stats
    let mining_stats = Arc::new(Mutex::new(stats::Mining::new()));

    // start HW backend for selected target
    hal::run(work_solver, mining_stats.clone(), shutdown_sender);
    // start statistics processing
    start_mining_stats_task(mining_stats);
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
                .value_name("HOSTNAME:PORT")
                .help("Address the stratum V2 server")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("user")
                .short("u")
                .long("user")
                .value_name("USERNAME.WORKERNAME")
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
