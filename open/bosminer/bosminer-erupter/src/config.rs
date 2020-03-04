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

use bosminer::client;
use bosminer::hal;

use bosminer_config::ClientDescriptor;

use std::time::Duration;

/// Override the default drain channel size as miner tends to burst messages into the logger
pub const ASYNC_LOGGER_DRAIN_CHANNEL_SIZE: usize = 128;

/// Number of midstates
pub const DEFAULT_MIDSTATE_COUNT: usize = 1;

/// Default hashrate interval used for statistics in seconds
pub const DEFAULT_HASHRATE_INTERVAL: Duration = Duration::from_secs(60);

/// Maximum time it takes to compute one job under normal circumstances
pub const JOB_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
pub struct Backend {
    client_manager: Option<client::Manager>,
    client_descriptor: Option<ClientDescriptor>,
}

impl Backend {
    pub fn new(client_descriptor: ClientDescriptor) -> Self {
        Self {
            client_manager: None,
            client_descriptor: Some(client_descriptor),
        }
    }

    pub async fn init_client(self) {
        if let Some(client_descriptor) = self.client_descriptor {
            let group = self
                .client_manager
                .expect("BUG: missing client manager")
                .create_or_get_default_group()
                .await;

            group
                .push_client(client::Handle::new(client_descriptor, None, None))
                .await;
        }
    }
}

impl hal::BackendConfig for Backend {
    #[inline]
    fn midstate_count(&self) -> usize {
        DEFAULT_MIDSTATE_COUNT
    }

    fn set_client_manager(&mut self, client_manager: client::Manager) {
        self.client_manager.replace(client_manager);
    }
}
