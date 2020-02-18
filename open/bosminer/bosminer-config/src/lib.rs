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

mod client;
mod error;
mod group;

// Reexport inner structures
pub use client::Descriptor as ClientDescriptor;
pub use client::Protocol as ClientProtocol;
pub use client::URL_JAVA_SCRIPT_REGEX as CLIENT_URL_JAVA_SCRIPT_REGEX;
pub use group::Descriptor as GroupDescriptor;

// reexport common crates
pub use clap;
pub use config;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct PoolConfig {
    pub url: String,
    pub user: String,
    pub password: Option<String>,
}

/// Parse a configuration file from `config_path`.
pub fn parse<'a, T>(config_path: &str) -> Result<T, String>
where
    T: Deserialize<'a>,
{
    let mut settings = config::Config::default();
    settings
        .merge(config::File::with_name(config_path))
        .map_err(|e| format!("{}", e))?;

    // Parse it into structure
    settings.try_into::<T>().map_err(|e| format!("{}", e))
}
