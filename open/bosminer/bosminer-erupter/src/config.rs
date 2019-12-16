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

use bosminer::hal;

use std::time::Duration;

/// Override the default drain channel size as miner tends to burst messages into the logger
pub const ASYNC_LOGGER_DRAIN_CHANNEL_SIZE: usize = 128;

/// Number of midstates
pub const DEFAULT_MIDSTATE_COUNT: usize = 1;

/// Default hashrate interval used for statistics in seconds
pub const DEFAULT_HASHRATE_INTERVAL: Duration = Duration::from_secs(60);

/// Maximum time it takes to compute one job under normal circumstances
pub const JOB_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct Backend {
    pub client: Option<bosminer_config::client::Descriptor>,
}

impl Default for Backend {
    fn default() -> Self {
        Self { client: None }
    }
}

impl hal::BackendConfig for Backend {
    #[inline]
    fn midstate_count(&self) -> usize {
        DEFAULT_MIDSTATE_COUNT
    }

    fn clients(&mut self) -> Vec<bosminer_config::client::Descriptor> {
        self.client
            .take()
            .map(|client| vec![client])
            .unwrap_or_default()
    }
}
