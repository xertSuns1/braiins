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
use support::{MultiResponse, Response, ResponseType, Timestamp};

mod response;

mod command;
use command::{Command, Handler};

mod server;

#[cfg(test)]
mod test;

use crate::hub;
use crate::node;

/// Version of CGMiner compatible API
const API_VERSION: &str = "3.7";

/// Default interval used for computation of default rolling average.
const DEFAULT_LOG_INTERVAL: u32 = 5;

/// Global `Timestamp` flag, controls whether responses contain real timestamps.
/// See also the `Timestamp` type.
static TIMESTAMP: Timestamp = Timestamp::new();

struct CGMinerAPI {
    core: Arc<hub::Core>,
}

impl CGMinerAPI {
    pub fn new(core: Arc<hub::Core>) -> Self {
        Self { core }
    }

    async fn get_pool_status(idx: usize, _client: &Arc<dyn node::Client>) -> response::Pool {
        response::Pool {
            idx: idx as u32,
            url: "".to_string(),
            status: response::PoolStatus::Alive,
            priority: 0,
            quota: 0,
            long_poll: response::Bool::N,
            getworks: 0,
            accepted: 0,
            rejected: 0,
            works: 0,
            discarded: 0,
            stale: 0,
            get_failures: 0,
            remote_failures: 0,
            user: "".to_string(),
            last_share_time: 0,
            diff1_shares: 0,
            proxy_type: "".to_string(),
            proxy: "".to_string(),
            difficulty_accepted: 0.0,
            difficulty_rejected: 0.0,
            difficulty_stale: 0.0,
            last_share_difficulty: 0.0,
            work_difficulty: 0.0,
            has_stratum: false,
            stratum_active: false,
            stratum_url: "".to_string(),
            stratum_difficulty: 0.0,
            has_vmask: false,
            has_gbt: false,
            best_share: 0,
            pool_rejected_percent: 0.0,
            pool_stale_percent: 0.0,
            bad_work: 0,
            current_block_height: 0,
            current_block_version: 0,
        }
    }

    async fn get_pool_statuses(&self) -> Vec<response::Pool> {
        let mut list = vec![];
        for (idx, client) in self.core.get_clients().await.iter().enumerate() {
            list.push(Self::get_pool_status(idx, client).await);
        }
        list
    }

    async fn get_asc_status(idx: usize, _work_solver: &Arc<dyn node::WorkSolver>) -> response::Asc {
        response::Asc {
            idx: idx as u32,
            // TODO: get actual ASIC name from work solver
            name: "BC5".to_string(),
            // TODO: get idx from work solver (it can represent real index of hash chain)
            id: idx as u32,
            // TODO: get actual state from work solver
            enabled: response::Bool::Y,
            // TODO: get actual status from work solver
            status: response::AscStatus::Alive,
            // TODO: get actual temperature from work solver?
            temperature: 0.0,
            mhs_av: 0.0,
            mhs_5s: 0.0,
            mhs_1m: 0.0,
            mhs_5m: 0.0,
            mhs_15m: 0.0,
            accepted: 0,
            rejected: 0,
            hardware_errors: 0,
            utility: 0.0,
            last_share_pool: 0,
            last_share_time: 0,
            total_mh: 0.0,
            diff1_work: 0,
            difficulty_accepted: 0.0,
            difficulty_rejected: 0.0,
            last_share_difficulty: 0.0,
            last_valid_work: 0,
            device_hardware_percent: 0.0,
            device_rejected_percent: 0.0,
            device_elapsed: 0,
        }
    }

    async fn get_asc_statuses(&self) -> Vec<response::Asc> {
        let mut list = vec![];
        for (idx, work_solver) in self.core.get_work_solvers().await.iter().enumerate() {
            list.push(Self::get_asc_status(idx, work_solver).await);
        }
        list
    }
}

#[async_trait::async_trait]
impl Handler for CGMinerAPI {
    async fn handle_pools(&self) -> command::Result<response::Pools> {
        Ok(response::Pools {
            list: self.get_pool_statuses().await,
        })
    }

    async fn handle_devs(&self) -> command::Result<response::Devs> {
        Ok(response::Devs {
            list: self.get_asc_statuses().await,
        })
    }

    async fn handle_edevs(&self) -> command::Result<response::Devs> {
        self.handle_devs().await
    }

    async fn handle_summary(&self) -> command::Result<response::Summary> {
        Ok(response::Summary {
            elapsed: 0,
            mhs_av: 0.0,
            mhs_5s: 0.0,
            mhs_1m: 0.0,
            mhs_5m: 0.0,
            mhs_15m: 0.0,
            found_blocks: 0,
            getworks: 0,
            accepted: 0,
            rejected: 0,
            hardware_errors: 0,
            utility: 0.0,
            discarded: 0,
            stale: 0,
            get_failures: 0,
            local_work: 0,
            remote_failures: 0,
            network_blocks: 0,
            total_mh: 0.0,
            work_utility: 0.0,
            difficulty_accepted: 0.0,
            difficulty_rejected: 0.0,
            difficulty_stale: 0.0,
            best_share: 0,
            device_hardware_percent: 0.0,
            device_rejected_percent: 0.0,
            pool_rejected_percent: 0.0,
            pool_stale_percent: 0.0,
            last_getwork: 0,
        })
    }

    async fn handle_version(&self) -> command::Result<response::Version> {
        Ok(response::Version {
            // TODO: get actual bosminer version
            cgminer: "bOSminer_am1-s9-20190605-0_0de55997".into(),
            api: API_VERSION.into(),
        })
    }

    async fn handle_config(&self) -> command::Result<response::Config> {
        Ok(response::Config {
            asc_count: self.core.get_work_solvers().await.len() as u32,
            pga_count: 0,
            pool_count: self.core.get_clients().await.len() as u32,
            // TODO: get actual multi-pool strategy
            strategy: response::MultipoolStrategy::Failover,
            log_interval: DEFAULT_LOG_INTERVAL,
            device_code: String::new(),
            // TODO: detect underlying operation system
            os: "Braiins OS".to_string(),
            hotplug: "None".to_string(),
        })
    }

    async fn handle_dev_details(&self) -> command::Result<response::DevDetails> {
        Ok(response::DevDetails {
            idx: 0,
            name: "".to_string(),
            id: 0,
            driver: "".to_string(),
            kernel: "".to_string(),
            model: "".to_string(),
            device_path: "".to_string(),
        })
    }
}

pub async fn run(core: Arc<hub::Core>, listen_addr: SocketAddr) {
    let handler = Arc::new(CGMinerAPI::new(core));
    server::run(handler, listen_addr).await.unwrap();
}
