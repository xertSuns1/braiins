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

pub struct BasicTest;

use crate::command;
use crate::response;

use serde_json as json;

#[async_trait::async_trait]
impl command::Handler for BasicTest {
    async fn handle_pools(&self) -> command::Result<response::Pools> {
        Ok(response::Pools {
            list: vec![response::Pool {
                idx: 0,
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
                pool_rejected_ratio: 0.0,
                pool_stale_ratio: 0.0,
                bad_work: 0,
                current_block_height: 0,
                current_block_version: 0,
                asic_boost: false,
            }],
        })
    }

    async fn handle_devs(&self) -> command::Result<response::Devs> {
        Ok(response::Devs {
            list: vec![response::Asc {
                idx: 0,
                name: "BC5".to_string(),
                id: 0,
                enabled: response::Bool::Y,
                status: response::AscStatus::Alive,
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
                total_mega_hashes: 0.0,
                diff1_work: 0,
                difficulty_accepted: 0.0,
                difficulty_rejected: 0.0,
                last_share_difficulty: 0.0,
                last_valid_work: 0,
                device_hardware_ratio: 0.0,
                device_rejected_ratio: 0.0,
                device_elapsed: 0,
                hardware_error_mhs_15m: 0.0,
                expected_mhs: 0.0,
            }],
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
            mhs_24h: 0.0,
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
            total_mega_hashes: 0.0,
            work_utility: 0.0,
            difficulty_accepted: 0.0,
            difficulty_rejected: 0.0,
            difficulty_stale: 0.0,
            best_share: 0,
            device_hardware_ratio: 0.0,
            device_rejected_ratio: 0.0,
            pool_rejected_ratio: 0.0,
            pool_stale_ratio: 0.0,
            last_getwork: 0,
        })
    }

    async fn handle_switch_pool(
        &self,
        _parameter: Option<&json::Value>,
    ) -> command::Result<response::SwitchPool> {
        Ok(response::SwitchPool {
            idx: 0,
            url: "".to_string(),
        })
    }

    async fn handle_config(&self) -> command::Result<response::Config> {
        Ok(response::Config {
            asc_count: 0,
            pga_count: 0,
            pool_count: 0,
            strategy: response::MultipoolStrategy::Failover,
            log_interval: 0,
            device_code: String::new(),
            os: "Braiins OS".to_string(),
            hotplug: "None".to_string(),
        })
    }

    async fn handle_add_pool(
        &self,
        _parameter: Option<&json::Value>,
    ) -> command::Result<response::AddPool> {
        Ok(response::AddPool {
            idx: 0,
            url: "".to_string(),
        })
    }

    async fn handle_remove_pool(
        &self,
        _parameter: Option<&json::Value>,
    ) -> command::Result<response::RemovePool> {
        Ok(response::RemovePool {
            idx: 0,
            url: "".to_string(),
        })
    }

    async fn handle_stats(&self) -> command::Result<response::Stats> {
        Ok(response::Stats {
            asc_stats: vec![response::AscStats {
                header: response::StatsHeader {
                    idx: 0,
                    id: "".to_string(),
                    elapsed: 0,
                    calls: 0,
                    wait: 0.0,
                    max: 0.0,
                    min: 0.0,
                },
            }],
            pool_stats: vec![response::PoolStats {
                header: response::StatsHeader {
                    idx: 0,
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
            }],
        })
    }

    async fn handle_estats(&self) -> command::Result<response::Stats> {
        Ok(response::Stats {
            asc_stats: vec![response::AscStats {
                header: response::StatsHeader {
                    idx: 0,
                    id: "".to_string(),
                    elapsed: 0,
                    calls: 0,
                    wait: 0.0,
                    max: 0.0,
                    min: 0.0,
                },
            }],
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
        Ok(response::AscCount { count: 0 })
    }

    async fn handle_asc(&self, _parameter: Option<&json::Value>) -> command::Result<response::Asc> {
        Ok(response::Asc {
            idx: 0,
            name: "BC5".to_string(),
            id: 0,
            enabled: response::Bool::Y,
            status: response::AscStatus::Alive,
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
            total_mega_hashes: 0.0,
            diff1_work: 0,
            difficulty_accepted: 0.0,
            difficulty_rejected: 0.0,
            last_share_difficulty: 0.0,
            last_valid_work: 0,
            device_hardware_ratio: 0.0,
            device_rejected_ratio: 0.0,
            device_elapsed: 0,
            hardware_error_mhs_15m: 0.0,
            expected_mhs: 0.0,
        })
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
