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

pub mod client;
pub mod error;

// reexport common crates
pub use clap;
pub use config;

use serde::Deserialize;

/// Expected configuration version
const CONFIG_VERSION: &'static str = "alpha";

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct PoolConfig {
    pub url: String,
    pub user: String,
    pub password: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GenericConfig {
    config_version: String,
    #[serde(rename = "pool")]
    pub pools: Option<Vec<PoolConfig>>,
    #[serde(flatten)]
    pub backend_config: config::Value,
}

/// Parse config (either specified or the default one)
pub fn parse(config_path: &str) -> GenericConfig {
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
