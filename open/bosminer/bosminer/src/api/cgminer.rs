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

mod command;
mod response;
mod server;
mod support;

#[cfg(test)]
mod test;

use crate::hub;
use crate::node;

use serde_json as json;

use crate::api::cgminer::support::ValueExt;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

/// Version of CGMiner compatible API
const API_VERSION: &str = "3.7";
/// Miner signature where `CGMiner` text is used to be
const SIGNATURE: &str = "bOSminer";

/// Default interval used for computation of default rolling average.
const DEFAULT_LOG_INTERVAL: u32 = 5;

/// Global `Timestamp` flag, controls whether responses contain real timestamps.
/// See also the `Timestamp` type.
static TIMESTAMP: support::Timestamp = support::Timestamp::new();

struct Handler {
    core: Arc<hub::Core>,
}

impl Handler {
    pub fn new(core: Arc<hub::Core>) -> Self {
        Self { core }
    }

    async fn collect_data<C, F, T, U, V>(&self, container: C, base_idx: usize, f: F) -> Vec<T>
    where
        C: Future<Output = Vec<Arc<U>>>,
        F: Fn(usize, Arc<U>) -> V,
        U: ?Sized,
        V: Future<Output = T>,
    {
        let mut list = vec![];
        for (idx, item) in container.await.iter().enumerate() {
            list.push(f(base_idx + idx, item.clone()).await);
        }
        list
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

    async fn collect_pool_statuses(&self) -> Vec<response::Pool> {
        self.collect_data(self.core.get_clients(), 0, |idx, client| {
            async move { Self::get_pool_status(idx, &client).await }
        })
        .await
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

    async fn collect_asc_statuses(&self) -> Vec<response::Asc> {
        self.collect_data(self.core.get_work_solvers(), 0, |idx, work_solver| {
            async move { Self::get_asc_status(idx, &work_solver).await }
        })
        .await
    }

    async fn get_pool_stats(idx: usize, _client: &Arc<dyn node::Client>) -> response::PoolStats {
        response::PoolStats {
            header: response::StatsHeader {
                idx: idx as u32,
                id: "".to_string(),
                elapsed: 0,
                calls: 0,
                wait: 0.0,
                max: 0.0,
                min: 0.0,
            },
            pool_calls: 0,
            pool_attempts: 0,
            pool_wait: 0.0,
            pool_max: 0.0,
            pool_min: 0.0,
            pool_av: 0.0,
            work_had_roll_time: false,
            work_can_roll: false,
            work_had_expire: false,
            work_roll_time: 0,
            work_diff: 0.0,
            min_diff: 0.0,
            max_diff: 0.0,
            min_diff_count: 0,
            max_diff_count: 0,
            times_sent: 0,
            bytes_sent: 0,
            times_recv: 0,
            bytes_recv: 0,
            net_bytes_sent: 0,
            net_bytes_recv: 0,
        }
    }

    async fn collect_pool_stats(&self, base_idx: usize) -> Vec<response::PoolStats> {
        self.collect_data(self.core.get_clients(), base_idx, |idx, client| {
            async move { Self::get_pool_stats(idx, &client).await }
        })
        .await
    }

    async fn get_asc_stats(
        idx: usize,
        _work_solver: &Arc<dyn node::WorkSolver>,
    ) -> response::AscStats {
        response::AscStats {
            header: response::StatsHeader {
                idx: idx as u32,
                id: "".to_string(),
                elapsed: 0,
                calls: 0,
                wait: 0.0,
                max: 0.0,
                min: 0.0,
            },
        }
    }

    async fn collect_asc_stats(&self, base_idx: usize) -> Vec<response::AscStats> {
        self.collect_data(
            self.core.get_work_solvers(),
            base_idx,
            |idx, work_solver| async move { Self::get_asc_stats(idx, &work_solver).await },
        )
        .await
    }
}

#[async_trait::async_trait]
impl command::Handler for Handler {
    async fn handle_pools(&self) -> command::Result<response::Pools> {
        Ok(response::Pools {
            list: self.collect_pool_statuses().await,
        })
    }

    async fn handle_devs(&self) -> command::Result<response::Devs> {
        Ok(response::Devs {
            list: self.collect_asc_statuses().await,
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
            miner: "bOSminer_am1-s9-20190605-0_0de55997".into(),
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

    async fn handle_stats(&self) -> command::Result<response::Stats> {
        let asc_stats = self.collect_asc_stats(0).await;
        let pool_stats = self.collect_pool_stats(asc_stats.len()).await;
        Ok(response::Stats {
            asc_stats,
            pool_stats,
        })
    }

    async fn handle_estats(&self) -> command::Result<response::Stats> {
        Ok(response::Stats {
            asc_stats: self.collect_asc_stats(0).await,
            pool_stats: vec![],
        })
    }

    async fn handle_coin(&self) -> command::Result<response::Coin> {
        Ok(response::Coin {
            hash_method: "".to_string(),
            current_block_time: 0.0,
            current_block_hash: "".to_string(),
            lp: false,
            network_difficulty: 0.0,
        })
    }

    async fn handle_asc_count(&self) -> command::Result<response::AscCount> {
        Ok(response::AscCount {
            count: self.core.get_work_solvers().await.len() as u32,
        })
    }

    async fn handle_asc(&self, parameter: Option<&json::Value>) -> command::Result<response::Asc> {
        let idx = parameter
            .expect("BUG: missing ASC parameter")
            .to_i32()
            .expect("BUG: invalid ASC parameter type");

        let work_solvers = self.core.get_work_solvers().await;
        let work_solver = work_solvers.get(idx as usize).cloned();

        match work_solver {
            Some(work_solver) => Ok(Self::get_asc_status(idx as usize, &work_solver).await),
            None => {
                Err(response::ErrorCode::InvalidAscId(idx, work_solvers.len() as i32 - 1).into())
            }
        }
    }

    async fn handle_lcd(&self) -> command::Result<response::Lcd> {
        Ok(response::Lcd {
            elapsed: 0,
            ghs_av: 0.0,
            ghs_5m: 0.0,
            ghs_5s: 0.0,
            temperature: 0.0,
            last_share_difficulty: 0.0,
            last_share_time: 0,
            best_share: 0,
            last_valid_work: 0,
            found_blocks: 0,
            current_pool: "".to_string(),
            user: "".to_string(),
        })
    }
}

pub async fn run(core: Arc<hub::Core>, listen_addr: SocketAddr) {
    let handler = Handler::new(core);
    server::run(handler, listen_addr).await.unwrap();
}
