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

use super::Response;

#[allow(dead_code)]
#[derive(Serialize, Eq, PartialEq, Clone, Debug)]
pub enum Bool {
    N,
    Y,
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

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Asc {
    #[serde(rename = "ASC")]
    pub asc: u32,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "ID")]
    pub id: u32,
    #[serde(rename = "Enabled")]
    pub enabled: Bool,
    #[serde(rename = "Status")]
    pub status: AscStatus,
    #[serde(rename = "Temperature")]
    pub temperature: f64,
    #[serde(rename = "MHS av")]
    pub mhs_av: f64,
    #[serde(rename = "MHS 5s")]
    pub mhs_5s: f64,
    #[serde(rename = "MHS 1m")]
    pub mhs_1m: f64,
    #[serde(rename = "MHS 5m")]
    pub mhs_5m: f64,
    #[serde(rename = "MHS 15m")]
    pub mhs_15m: f64,
    #[serde(rename = "Accepted")]
    pub accepted: u32,
    #[serde(rename = "Rejected")]
    pub rejected: u32,
    #[serde(rename = "Hardware Errors")]
    pub hardware_errors: u32,
    #[serde(rename = "Utility")]
    pub utility: f64,
    #[serde(rename = "Last Share Pool")]
    pub last_share_pool: u32,
    #[serde(rename = "Last Share Time")]
    pub last_share_time: u32,
    #[serde(rename = "Total MH")]
    pub total_mh: f64,
    #[serde(rename = "Diff1 Work")]
    pub diff1_work: u64,
    #[serde(rename = "Difficulty Accepted")]
    pub difficulty_accepted: f64,
    #[serde(rename = "Difficulty Rejected")]
    pub difficulty_rejected: f64,
    #[serde(rename = "Last Share Difficulty")]
    pub last_share_difficulty: f64,
    #[serde(rename = "Last Valid Work")]
    pub last_valid_work: u32,
    #[serde(rename = "Device Hardware%")]
    pub device_hardware_percent: f64,
    #[serde(rename = "Device Rejected%")]
    pub device_rejected_percent: f64,
    #[serde(rename = "Device Elapsed")]
    pub device_elapsed: u32,
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Devs {
    pub list: Vec<Asc>,
}

impl From<Devs> for Response {
    fn from(devs: Devs) -> Response {
        let asc_count = devs.list.len();
        Response::new(devs.list, "DEVS", true, 9, format!("{} ASC(s)", asc_count))
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
            22,
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
            33,
            "CGMiner config".to_string(),
        )
    }
}
