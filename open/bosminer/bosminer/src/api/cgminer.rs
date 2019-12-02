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

//! This module implements CGMiner compatible API server to control bOSminer and to extract
//! statistics from it.

use std::net::SocketAddr;
use std::sync::Arc;

mod support;
use support::{MultiResponse, Response, ResponseSet, Timestamp};

mod response;

mod command;
use command::{Command, Handler};

mod server;

#[cfg(test)]
mod test;

/// Global `Timestamp` flag, controls whether responses contain real timestamps.
/// See also the `Timestamp` type.
static TIMESTAMP: Timestamp = Timestamp::new();

struct CGMinerAPI;

#[async_trait::async_trait]
impl Handler for CGMinerAPI {
    async fn handle_version(&self) -> Option<Response> {
        let version = response::Version {
            cgminer: "bOSminer_am1-s9-20190605-0_0de55997".into(),
            api: "3.7".into(),
        };

        Some(version.into())
    }

    async fn handle_config(&self) -> Option<Response> {
        let config = response::Config {
            asc_count: 0,
            pga_count: 0,
            pool_count: 0,
            strategy: "Failover".to_string(),
            log_interval: 0,
            device_code: String::new(),
            os: "Braiins OS".to_string(),
            hotplug: "None".to_string(),
        };

        Some(config.into())
    }
}

pub async fn run(listen_addr: SocketAddr) {
    let handler = Arc::new(CGMinerAPI);
    server::run(handler, listen_addr).await.unwrap();
}
