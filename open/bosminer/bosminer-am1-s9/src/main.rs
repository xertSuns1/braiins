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

use ii_logging::macros::*;

use bosminer::hal;
use bosminer_am1_s9::config;
use bosminer_config::clap;

use ii_async_compat::tokio;

#[tokio::main]
async fn main() {
    let app = clap::App::new("bosminer")
        .version(bosminer::version::STRING.as_str())
        .arg(
            clap::Arg::with_name("config")
                .long("config")
                .help("Set config file path")
                .required(false)
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("pool")
                .short("p")
                .long("pool")
                .value_name("HOSTNAME:PORT")
                .help("Address the stratum V2 server")
                .required(false)
                .requires("user")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("user")
                .short("u")
                .long("user")
                .value_name("USERNAME.WORKERNAME")
                .help("Specify user and worker name")
                .required(false)
                .requires("pool")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("disable-asic-boost")
                .long("disable-asic-boost")
                .help("Disable ASIC boost (use just one midstate)")
                .required(false),
        )
        .arg(
            clap::Arg::with_name("frequency")
                .long("frequency")
                .help("Set chip frequency (in MHz)")
                .required(false)
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("voltage")
                .long("voltage")
                .help("Set chip voltage (in volts)")
                .required(false)
                .takes_value(true),
        )
        .subcommand(
            clap::SubCommand::with_name("config")
                .about("Configuration backend API")
                .version("beta")
                .arg(
                    clap::Arg::with_name("metadata")
                        .long("metadata")
                        .help("Handle 'metadata' request and write result to stdout")
                        .required(false)
                        .takes_value(false),
                )
                .arg(
                    clap::Arg::with_name("data")
                        .long("data")
                        .help("Handle 'data' request and write result to stdout")
                        .required(false)
                        .takes_value(false),
                )
                .arg(
                    clap::Arg::with_name("save")
                        .long("save")
                        .help("Handle 'save' request from stdin and write result to stdout")
                        .required(false)
                        .takes_value(false),
                )
                .group(
                    clap::ArgGroup::with_name("command")
                        .args(&["metadata", "data", "save"])
                        .required(true),
                ),
        );

    let matches = app.get_matches();
    let _log_guard =
        ii_logging::setup_for_app(bosminer_am1_s9::config::ASYNC_LOGGER_DRAIN_CHANNEL_SIZE);

    let config_path = matches
        .value_of("config")
        .unwrap_or(config::DEFAULT_CONFIG_PATH);

    // Handle special 'config' sub-command available for configuration backend API
    if let Some(matches) = matches.subcommand_matches("config") {
        let config_handler = config::api::Handler::new(config_path);
        if matches.is_present("metadata") {
            config_handler.handle_metadata();
        } else if matches.is_present("data") {
            config_handler.handle_data();
        } else if matches.is_present("save") {
            config_handler.handle_save();
        }
        return;
    }

    let mut backend_config = match config::Backend::parse(config_path) {
        Err(e) => {
            error!("Cannot load configuration file \"{}\"", config_path);
            error!("Reason: {}", e);
            return;
        }
        Ok(v) => v,
    };

    // Add pools from command line
    if let Some(url) = matches.value_of("pool") {
        let user = matches
            .value_of("user")
            .expect("BUG: missing 'user' argument");

        let client_descriptor =
            bosminer_config::ClientDescriptor::parse(url, user).expect("Server parameters");
        let group_config = hal::GroupConfig {
            descriptor: Default::default(),
            clients: vec![hal::ClientConfig {
                descriptor: client_descriptor,
                channel: None,
            }],
        };

        if !backend_config.client_groups.is_empty() {
            warn!("Overriding pool settings located at '{}'", config_path);
        }

        backend_config.client_groups = vec![group_config];
    }

    // Check if there's enough pools
    if backend_config.client_groups.len() == 0 {
        error!("No pools specified!");
        info!("Use cli arguments:");
        info!("    bosminer --pool <HOSTNAME:PORT> --user <USERNAME.WORKERNAME[:PASSWORD]>");
        info!(
            "Or specify pool(s) in configuration file '{}':",
            config_path
        );
        info!("    in [[pool]] section");
        return;
    }

    // Set just 1 midstate if user requested disabling asicboost
    if matches.is_present("disable-asic-boost") {
        backend_config
            .hash_chain_global
            .get_or_insert_with(|| Default::default())
            .asic_boost
            .replace(false);
    }
    if let Some(value) = matches.value_of("frequency") {
        let frequency = value.parse::<f64>().expect("not a float number");
        backend_config
            .hash_chain_global
            .get_or_insert_with(|| Default::default())
            .overridable
            .get_or_insert_with(|| Default::default())
            .frequency
            .replace(frequency);
    }
    if let Some(value) = matches.value_of("voltage") {
        let voltage = value.parse::<f64>().expect("not a float number");
        backend_config
            .hash_chain_global
            .get_or_insert_with(|| Default::default())
            .overridable
            .get_or_insert_with(|| Default::default())
            .voltage
            .replace(voltage);
    }

    ii_async_compat::setup_panic_handling();
    bosminer::main::<bosminer_am1_s9::Backend>(backend_config).await;
}
