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

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Version {
    #[serde(rename = "CGMiner")]
    pub cgminer: String,
    #[serde(rename = "API")]
    pub api: String,
}

impl From<Version> for Response {
    fn from(ver: Version) -> Response {
        Response::new(ver, "VERSION", true, 22, "CGMiner versions".to_string())
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
    pub strategy: String,
    #[serde(rename = "Log Interval")]
    pub log_interval: u32,
    #[serde(rename = "Device Code:")]
    pub device_code: String,
    #[serde(rename = "OS")]
    pub os: String,
    #[serde(rename = "Hotplug")]
    pub hotplug: String,
}

impl From<Config> for Response {
    fn from(ver: Config) -> Response {
        Response::new(ver, "CONFIG", true, 33, "CGMiner config".to_string())
    }
}
