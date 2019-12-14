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
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("user")
                .short("u")
                .long("user")
                .value_name("USERNAME.WORKERNAME")
                .help("Specify user and worker name")
                .required(false)
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
        );

    let matches = app.get_matches();

    let config_path = matches
        .value_of("config")
        .unwrap_or(config::DEFAULT_CONFIG_PATH);
    let mut backend_config = config::Backend::parse(config_path);

    // Add pools from command line
    if let Some(url) = matches.value_of("pool") {
        let user = matches.value_of("user").expect("missing 'user' argument");

        let client_descriptor = bosminer_config::client::parse(url.to_string(), user.to_string())
            .expect("Server parameters");

        backend_config.clients.push(client_descriptor);
    }

    // Check if there's enough pools
    if backend_config.clients.len() == 0 {
        panic!("No pools specified.");
    }

    // Set just 1 midstate if user requested disabling asicboost
    if matches.is_present("disable-asic-boost") {
        backend_config.asic_boost = false;
    }
    if let Some(value) = matches.value_of("frequency") {
        backend_config.frequency = value.parse::<f32>().expect("not a float number");
    }
    if let Some(value) = matches.value_of("voltage") {
        backend_config.voltage = value.parse::<f32>().expect("not a float number");
    }

    ii_async_compat::setup_panic_handling();
    bosminer::main::<bosminer_am1_s9::Backend>(backend_config).await;
}
