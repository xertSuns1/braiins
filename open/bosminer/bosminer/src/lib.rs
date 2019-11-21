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

mod api;
pub mod backend;
pub mod client;
pub mod config;
pub mod entry;
pub mod error;
pub mod hal;
pub mod hub;
pub mod job;
pub mod node;
pub mod runtime_config;
pub mod stats;
pub mod work;

pub mod test_utils;

// reexport main function from `entry` module
pub use entry::main;
// reexport clap which is needed in `hal::Backend::add_args`
pub use clap;

use bosminer_macros::MiningNode;

use std::fmt;
use std::sync::Arc;

use once_cell::sync::Lazy;

#[derive(Debug, MiningNode)]
pub struct Frontend {
    #[member_mining_stats]
    mining_stats: stats::BasicMining,
}

impl Frontend {
    pub fn new() -> Self {
        Self {
            mining_stats: Default::default(),
        }
    }
}

impl fmt::Display for Frontend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bOSminer")
    }
}

/// Shared (global) configuration structure
pub static BOSMINER: Lazy<Arc<Frontend>> = Lazy::new(|| Arc::new(Frontend::new()));
