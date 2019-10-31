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

//! This module provides top level functionality to parse command line options and run the
//! mining protocol client (= bosminer frontend) and connect it with provided hardware
//! specific backend.

use crate::client;
use crate::hal;
use crate::hub;
use crate::runtime_config;
use crate::shutdown;
use crate::stats;
use crate::BOSMINER;

use ii_async_compat::tokio;

use std::sync::Arc;

use clap::{self, Arg};

pub async fn main<T: hal::Backend>(mut backend: T) {
    let _log_guard = ii_logging::setup_for_app();

    let app = clap::App::new("bosminer")
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
        );

    // Pass pre-build arguments to backend for further modification
    let args = backend.add_args(app).get_matches();

    // Unwraps should be ok as long as the flags are required
    let url = args.value_of("pool").unwrap().to_string();
    let user = args.value_of("user").unwrap().to_string();

    // parse user input to fail fast when it is incorrect
    let client_descriptor = client::parse(url, user).expect("Server parameters");

    // Set default backend midstate count
    runtime_config::set_midstate_count(T::DEFAULT_MIDSTATE_COUNT);

    // Allow backend to initialize itself with cli arguments
    backend.init(&args);

    // create job and work solvers
    let backend = Arc::new(backend);
    let (job_solver, work_solver) = hub::build_solvers(BOSMINER.clone(), backend.clone()).await;
    // create shutdown channel
    let (shutdown_sender, _shutdown_receiver) = shutdown::channel();

    // start HW backend for selected target
    backend.run(work_solver, shutdown_sender);

    // start statistics processing
    tokio::spawn(stats::mining_task(
        BOSMINER.clone(),
        T::DEFAULT_HASHRATE_INTERVAL,
    ));
    // start client based on user input
    client::run(job_solver, client_descriptor).await;
}
