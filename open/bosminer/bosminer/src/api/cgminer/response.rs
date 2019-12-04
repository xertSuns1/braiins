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

//! Defines all the CGMiner API responses

use serde::Serialize;
use serde_repr::Serialize_repr;

use super::Response;

pub type Time = u32;
pub type Elapsed = u32;
pub type Percent = f64;
pub type Difficulty = f64;
pub type MegaHashes = f64;
pub type TotalMegaHashes = f64;
pub type Utility = f64;
pub type Temperature = f64;

#[allow(dead_code)]
/// CGMiner API Status indicator.
/// (warning and info levels not currently used.)
#[derive(Serialize, Eq, PartialEq, Clone, Debug)]
pub enum Status {
    W,
    I,
    S,
    E,
}

#[allow(dead_code)]
#[derive(Serialize, Eq, PartialEq, Clone, Debug)]
pub enum Bool {
    N,
    Y,
}

#[allow(dead_code)]
#[derive(Serialize, Eq, PartialEq, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub enum PoolStatus {
    Disabled,
    Rejecting,
    Dead,
    Alive,
    Unknown,
}

#[allow(dead_code)]
#[derive(Serialize, Eq, PartialEq, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub enum AscStatus {
    Alive,
    Sick,
    Dead,
    NoStart,
    Initialising,
    Unknown,
}

#[allow(dead_code)]
#[derive(Serialize, Eq, PartialEq, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub enum MultipoolStrategy {
    Failover,
    #[serde(rename = "Round Robin")]
    RoundRobin,
    Rotate,
    #[serde(rename = "Load Balance")]
    LoadBalance,
    Balance,
}

#[derive(Serialize_repr, Eq, PartialEq, Clone, Debug)]
#[repr(u32)]
pub enum StatusCode {
    Pool = 7,
    Devs = 9,
    Summary = 11,
    Version = 22,
    MineConfig = 33,
    DevDetails = 69,
}

/// STATUS structure present in all replies
#[derive(Serialize, PartialEq, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct StatusInfo {
    #[serde(rename = "STATUS")]
    pub status: Status,
    pub when: Time,
    pub code: StatusCode,
    pub msg: String,
    pub description: String,
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Pool {
    #[serde(rename = "POOL")]
    pub idx: u32,
    #[serde(rename = "URL")]
    pub url: String,
    #[serde(rename = "Status")]
    pub status: PoolStatus,
    #[serde(rename = "Priority")]
    pub priority: u32,
    #[serde(rename = "Quota")]
    pub quota: u32,
    #[serde(rename = "Long Poll")]
    pub long_poll: Bool,
    #[serde(rename = "Getworks")]
    pub getworks: u32,
    #[serde(rename = "Accepted")]
    pub accepted: u64,
    #[serde(rename = "Rejected")]
    pub rejected: u64,
    #[serde(rename = "Works")]
    pub works: u32,
    #[serde(rename = "Discarded")]
    pub discarded: u32,
    #[serde(rename = "Stale")]
    pub stale: u32,
    #[serde(rename = "Get Failures")]
    pub get_failures: u32,
    #[serde(rename = "Remote Failures")]
    pub remote_failures: u32,
    #[serde(rename = "User")]
    pub user: String,
    #[serde(rename = "Last Share Time")]
    pub last_share_time: Time,
    #[serde(rename = "Diff1 Shares")]
    pub diff1_shares: u64,
    #[serde(rename = "Proxy Type")]
    pub proxy_type: String,
    #[serde(rename = "Proxy")]
    pub proxy: String,
    #[serde(rename = "Difficulty Accepted")]
    pub difficulty_accepted: Difficulty,
    #[serde(rename = "Difficulty Rejected")]
    pub difficulty_rejected: Difficulty,
    #[serde(rename = "Difficulty Stale")]
    pub difficulty_stale: Difficulty,
    #[serde(rename = "Last Share Difficulty")]
    pub last_share_difficulty: Difficulty,
    #[serde(rename = "Work Difficulty")]
    pub work_difficulty: Difficulty,
    #[serde(rename = "Has Stratum")]
    pub has_stratum: bool,
    #[serde(rename = "Stratum Active")]
    pub stratum_active: bool,
    #[serde(rename = "Stratum URL")]
    pub stratum_url: String,
    #[serde(rename = "Stratum Difficulty")]
    pub stratum_difficulty: Difficulty,
    #[serde(rename = "Has Vmask")]
    pub has_vmask: bool,
    #[serde(rename = "Has GBT")]
    pub has_gbt: bool,
    #[serde(rename = "Best Share")]
    pub best_share: u64,
    #[serde(rename = "Pool Rejected%")]
    pub pool_rejected_percent: Percent,
    #[serde(rename = "Pool Stale%")]
    pub pool_stale_percent: Percent,
    #[serde(rename = "Bad Work")]
    pub bad_work: u64,
    #[serde(rename = "Current Block Height")]
    pub current_block_height: u32,
    #[serde(rename = "Current Block Version")]
    pub current_block_version: u32,
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Pools {
    pub list: Vec<Pool>,
}

impl From<Pools> for Response {
    fn from(pools: Pools) -> Response {
        let pool_count = pools.list.len();
        Response::new(
            pools.list,
            "POOLS",
            true,
            StatusCode::Pool,
            format!("{} Pool(s)", pool_count),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Asc {
    #[serde(rename = "ASC")]
    pub idx: u32,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "ID")]
    pub id: u32,
    #[serde(rename = "Enabled")]
    pub enabled: Bool,
    #[serde(rename = "Status")]
    pub status: AscStatus,
    #[serde(rename = "Temperature")]
    pub temperature: Temperature,
    #[serde(rename = "MHS av")]
    pub mhs_av: MegaHashes,
    #[serde(rename = "MHS 5s")]
    pub mhs_5s: MegaHashes,
    #[serde(rename = "MHS 1m")]
    pub mhs_1m: MegaHashes,
    #[serde(rename = "MHS 5m")]
    pub mhs_5m: MegaHashes,
    #[serde(rename = "MHS 15m")]
    pub mhs_15m: MegaHashes,
    #[serde(rename = "Accepted")]
    pub accepted: u32,
    #[serde(rename = "Rejected")]
    pub rejected: u32,
    #[serde(rename = "Hardware Errors")]
    pub hardware_errors: u32,
    #[serde(rename = "Utility")]
    pub utility: Utility,
    #[serde(rename = "Last Share Pool")]
    pub last_share_pool: u32,
    #[serde(rename = "Last Share Time")]
    pub last_share_time: Time,
    #[serde(rename = "Total MH")]
    pub total_mh: TotalMegaHashes,
    #[serde(rename = "Diff1 Work")]
    pub diff1_work: u64,
    #[serde(rename = "Difficulty Accepted")]
    pub difficulty_accepted: Difficulty,
    #[serde(rename = "Difficulty Rejected")]
    pub difficulty_rejected: Difficulty,
    #[serde(rename = "Last Share Difficulty")]
    pub last_share_difficulty: Difficulty,
    #[serde(rename = "Last Valid Work")]
    pub last_valid_work: Time,
    #[serde(rename = "Device Hardware%")]
    pub device_hardware_percent: Percent,
    #[serde(rename = "Device Rejected%")]
    pub device_rejected_percent: Percent,
    #[serde(rename = "Device Elapsed")]
    pub device_elapsed: Elapsed,
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Devs {
    pub list: Vec<Asc>,
}

impl From<Devs> for Response {
    fn from(devs: Devs) -> Response {
        let asc_count = devs.list.len();
        Response::new(
            devs.list,
            "DEVS",
            true,
            StatusCode::Devs,
            format!("{} ASC(s)", asc_count),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Summary {
    #[serde(rename = "Elapsed")]
    pub elapsed: Elapsed,
    #[serde(rename = "MHS av")]
    pub mhs_av: MegaHashes,
    #[serde(rename = "MHS 5s")]
    pub mhs_5s: MegaHashes,
    #[serde(rename = "MHS 1m")]
    pub mhs_1m: MegaHashes,
    #[serde(rename = "MHS 5m")]
    pub mhs_5m: MegaHashes,
    #[serde(rename = "MHS 15m")]
    pub mhs_15m: MegaHashes,
    #[serde(rename = "Found Blocks")]
    pub found_blocks: u32,
    #[serde(rename = "Getworks")]
    pub getworks: u64,
    #[serde(rename = "Accepted")]
    pub accepted: u64,
    #[serde(rename = "Rejected")]
    pub rejected: u64,
    #[serde(rename = "Hardware Errors")]
    pub hardware_errors: u32,
    #[serde(rename = "Utility")]
    pub utility: Utility,
    #[serde(rename = "Discarded")]
    pub discarded: u64,
    #[serde(rename = "Stale")]
    pub stale: u64,
    #[serde(rename = "Get Failures")]
    pub get_failures: u32,
    #[serde(rename = "Local Work")]
    pub local_work: u32,
    #[serde(rename = "Remote Failures")]
    pub remote_failures: u32,
    #[serde(rename = "Network Blocks")]
    pub network_blocks: u32,
    #[serde(rename = "Total MH")]
    pub total_mh: TotalMegaHashes,
    #[serde(rename = "Work Utility")]
    pub work_utility: Utility,
    #[serde(rename = "Difficulty Accepted")]
    pub difficulty_accepted: Difficulty,
    #[serde(rename = "Difficulty Rejected")]
    pub difficulty_rejected: Difficulty,
    #[serde(rename = "Difficulty Stale")]
    pub difficulty_stale: Difficulty,
    #[serde(rename = "Best Share")]
    pub best_share: u64,
    #[serde(rename = "Device Hardware%")]
    pub device_hardware_percent: Percent,
    #[serde(rename = "Device Rejected%")]
    pub device_rejected_percent: Percent,
    #[serde(rename = "Pool Rejected%")]
    pub pool_rejected_percent: Percent,
    #[serde(rename = "Pool Stale%")]
    pub pool_stale_percent: Percent,
    #[serde(rename = "Last getwork")]
    pub last_getwork: Time,
}

impl From<Summary> for Response {
    fn from(summary: Summary) -> Response {
        Response::new(
            vec![summary],
            "SUMMARY",
            true,
            StatusCode::Summary,
            "Summary".to_string(),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Version {
    #[serde(rename = "CGMiner")]
    pub cgminer: String,
    #[serde(rename = "API")]
    pub api: String,
}

impl From<Version> for Response {
    fn from(version: Version) -> Response {
        Response::new(
            vec![version],
            "VERSION",
            true,
            StatusCode::Version,
            "CGMiner versions".to_string(),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Config {
    #[serde(rename = "ASC Count")]
    pub asc_count: u32,
    #[serde(rename = "PGA Count")]
    pub pga_count: u32,
    #[serde(rename = "Pool Count")]
    pub pool_count: u32,
    #[serde(rename = "Strategy")]
    pub strategy: MultipoolStrategy,
    #[serde(rename = "Log Interval")]
    pub log_interval: u32,
    #[serde(rename = "Device Code")]
    pub device_code: String,
    #[serde(rename = "OS")]
    pub os: String,
    #[serde(rename = "Hotplug")]
    pub hotplug: String,
}

impl From<Config> for Response {
    fn from(config: Config) -> Response {
        Response::new(
            vec![config],
            "CONFIG",
            true,
            StatusCode::MineConfig,
            "CGMiner config".to_string(),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct DevDetails {
    #[serde(rename = "DEVDETAILS")]
    pub idx: u32,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "ID")]
    pub id: u32,
    #[serde(rename = "Driver")]
    pub driver: String,
    #[serde(rename = "Kernel")]
    pub kernel: String,
    #[serde(rename = "Model")]
    pub model: String,
    #[serde(rename = "Device Path")]
    pub device_path: String,
}

impl From<DevDetails> for Response {
    fn from(dev_details: DevDetails) -> Response {
        Response::new(
            vec![dev_details],
            "DEVDETAILS",
            true,
            StatusCode::DevDetails,
            "Device Details".to_string(),
        )
    }
}
