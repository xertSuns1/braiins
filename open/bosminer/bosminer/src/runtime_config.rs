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

//! This module manages bosminer runtime configuration
//! It will be refactored into something more flexible once we decide on how to manage the
//! configuration.

use crate::config;
use lazy_static::lazy_static;
use std::sync::Mutex;

/// Structure representing miner configuration
pub struct RunTimeConfig {
    pub midstate_count: usize,
}

impl RunTimeConfig {
    pub fn new() -> Self {
        Self {
            midstate_count: config::DEFAULT_MIDSTATE_COUNT,
        }
    }
}

lazy_static! {
    /// Shared (global) configuration structure
    static ref CONFIG: Mutex<RunTimeConfig> = Mutex::new(RunTimeConfig::new());
}

/// These functions are only temporary, until we unify midstate_count passing
pub fn set_midstate_count(value: usize) {
    CONFIG.lock().expect("config lock").midstate_count = value;
}

pub fn get_midstate_count() -> usize {
    CONFIG.lock().expect("config lock").midstate_count
}
