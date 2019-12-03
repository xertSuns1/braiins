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

use crate::api;
use crate::client;
use crate::hal;
use crate::hub;
use crate::runtime_config;
use crate::stats;

use serde::Deserialize;

use ii_async_compat::tokio;

use std::sync::Arc;

use clap::{self, Arg};

/// Location of default config
/// TODO: Maybe don't add `.toml` prefix so we could use even JSON
pub const DEFAULT_CONFIG_PATH: &'static str = "/etc/bosminer/bosminer.toml";

/// Expected configuration version
const CONFIG_VERSION: &'static str = "alpha";

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct PoolConfig {
    url: String,
    user: String,
    password: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GenericConfig {
    config_version: String,
    #[serde(rename = "pool")]
    pools: Option<Vec<PoolConfig>>,
    #[serde(flatten)]
    pub backend_config: ::config::Value,
}

/// Registration is run in separate task to avoid blocking while the backend is being e.g. started
async fn backend_registration_task<T: hal::Backend>(
    core: Arc<hub::Core>,
    args: clap::ArgMatches<'_>,
    backend_config: ::config::Value,
) {
    core.add_backend::<T>(args, backend_config).await
}

/// Parse config (either specified or the default one)
pub fn parse_config(config_path: &str) -> GenericConfig {
    let mut settings = config::Config::default();
    settings
        .merge(config::File::with_name(config_path))
        .expect("failed to parse config file");

    // Parse it into structure
    let generic_config = settings
        .try_into::<GenericConfig>()
        .expect("failed to interpret config");

    // Check config is of the correct version
    if generic_config.config_version != CONFIG_VERSION {
        panic!("config_version should be {}", CONFIG_VERSION);
    }

    generic_config
}

pub async fn main<T: hal::Backend>() {
    let _log_guard = ii_logging::setup_for_app();

    let app = clap::App::new("bosminer")
        .arg(
            clap::Arg::with_name("config")
                .long("config")
                .help("Set config file path")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("pool")
                .short("p")
                .long("pool")
                .value_name("HOSTNAME:PORT")
                .help("Address the stratum V2 server")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("user")
                .short("u")
                .long("user")
                .value_name("USERNAME.WORKERNAME")
                .help("Specify user and worker name")
                .required(false)
                .takes_value(true),
        );

    // Pass pre-build arguments to backend for further modification
    let matches = T::add_args(app).get_matches();

    // Parse config file - either user specified or the default one
    let mut generic_config =
        parse_config(matches.value_of("config").unwrap_or(DEFAULT_CONFIG_PATH));

    // Parse pools
    // Don't worry if is this section missing, maybe there are some pools on command line
    let mut pools = generic_config.pools.take().unwrap_or_else(|| Vec::new());

    // Add pools from command line
    if let Some(url) = matches.value_of("pool") {
        if let Some(user) = matches.value_of("user") {
            pools.push(PoolConfig {
                url: url.to_string(),
                user: user.to_string(),
                password: None,
            });
        }
    }

    // Check if there's enough pools
    if pools.len() == 0 {
        panic!("No pools specified.");
    }

    // parse user input to fail fast when it is incorrect
    // TODO: insert here pool insertion && processing
    let pool = &pools[0]; // Whoa!
    let client_descriptor =
        client::parse(pool.url.clone(), pool.user.clone()).expect("Server parameters");

    // Set default backend midstate count
    runtime_config::set_midstate_count(T::DEFAULT_MIDSTATE_COUNT);

    // Initialize hub core which manages all resources
    let core = Arc::new(hub::Core::new());

    tokio::spawn(core.clone().run());
    tokio::spawn(backend_registration_task::<T>(
        core.clone(),
        matches,
        generic_config.backend_config,
    ));

    // start statistics processing
    tokio::spawn(stats::mining_task(
        core.frontend.clone(),
        T::DEFAULT_HASHRATE_INTERVAL,
    ));

    // start client based on user input
    client::register(&core, client_descriptor).await.enable();

    // the bosminer is controlled with API which also controls when the miner will end
    api::run(core).await;
}
