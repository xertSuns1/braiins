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

//! This module handles S9 configuration and configuration file parsing

use crate::bm1387::MidstateCount;
use crate::fan;
use crate::monitor;
use crate::power;
use crate::FrequencySettings;

use bosminer::hal::{self, BackendConfig as _};

use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

/// Location of default config
/// TODO: Maybe don't add `.toml` prefix so we could use even JSON
pub const DEFAULT_CONFIG_PATH: &'static str = "/etc/bosminer/bosminer.toml";

/// Default number of midstates
pub const DEFAULT_MIDSTATE_COUNT: usize = 4;

/// Default PLL frequency for clocking the chips
pub const DEFAULT_PLL_FREQUENCY: usize = 650_000_000;

/// Default voltage
pub const DEFAULT_VOLTAGE: f32 = 8.8;

/// Index of hashboard that is to be instantiated
pub const S9_HASHBOARD_INDEX: usize = 8;

/// Default ASIC difficulty
pub const ASIC_DIFFICULTY: usize = 64;

/// Default hashrate interval used for statistics in seconds
pub const DEFAULT_HASHRATE_INTERVAL: Duration = Duration::from_secs(60);

/// Maximum time it takes to compute one job under normal circumstances
pub const JOB_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct TempConfig {
    dangerous_temp: f32,
    hot_temp: f32,
}

impl Default for TempConfig {
    fn default() -> Self {
        Self {
            dangerous_temp: 105.0,
            hot_temp: 95.0,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct FanConfig {
    temperature: Option<f32>,
    speed: Option<usize>,
    min_fans: usize,
}

impl Default for FanConfig {
    fn default() -> Self {
        Self {
            temperature: Some(75.0),
            speed: None,
            min_fans: 1,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct ChainConfig {
    frequency: Option<f32>,
    voltage: Option<f32>,
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            frequency: Some(650.0),
            voltage: Some(9.0),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Backend {
    #[serde(skip)]
    pub clients: Vec<bosminer_config::client::Descriptor>,
    pub frequency: f32,
    pub voltage: f32,
    pub asic_boost: bool,
    temperature: Option<TempConfig>,
    fans: Option<FanConfig>,
    chain: Option<HashMap<String, ChainConfig>>,
}

impl Default for Backend {
    fn default() -> Self {
        Self {
            clients: vec![],
            frequency: 650.0,
            voltage: 9.0,
            asic_boost: true,
            temperature: Some(Default::default()),
            fans: Some(Default::default()),
            chain: None,
        }
    }
}

pub struct ResolvedChainConfig {
    pub midstate_count: MidstateCount,
    pub frequency: FrequencySettings,
    pub voltage: power::Voltage,
}

impl Backend {
    pub fn resolve_chain_config(&self, hashboard_idx: usize) -> ResolvedChainConfig {
        // take top-level configuration or default value
        let mut frequency = self.frequency;
        let mut voltage = self.voltage;

        // if there's a per-chain override then apply it
        if let Some(chain) = self
            .chain
            .as_ref()
            .and_then(|m| m.get(&hashboard_idx.to_string()))
        {
            frequency = chain.frequency.unwrap_or(frequency);
            voltage = chain.voltage.unwrap_or(voltage);
        }

        // computed s9-specific values
        ResolvedChainConfig {
            midstate_count: MidstateCount::new(self.midstate_count()),
            frequency: FrequencySettings::from_frequency((frequency * 1_000_000.0) as usize),
            voltage: power::Voltage::from_volts(voltage),
        }
    }

    pub fn resolve_monitor_config(&self) -> monitor::Config {
        let monitor_temp_config = if let Some(temp_config) = self.temperature.as_ref() {
            Some(monitor::TempControlConfig {
                dangerous_temp: temp_config.dangerous_temp,
                hot_temp: temp_config.hot_temp,
            })
        } else {
            None
        };
        let monitor_fan_config = if let Some(fan_config) = self.fans.as_ref() {
            let mode = if let Some(target_temp) = fan_config.temperature {
                if fan_config.speed.is_some() {
                    panic!("fan control: cannot specify both target temperature and target speed");
                }
                if monitor_temp_config.is_none() {
                    panic!(
                        "fan control: cannot specify target temperature with temp control disabled"
                    );
                }
                monitor::FanControlMode::TargetTemperature(target_temp)
            } else {
                if let Some(speed) = fan_config.speed {
                    monitor::FanControlMode::FixedSpeed(fan::Speed::new(speed))
                } else {
                    panic!("fan control: you have to specify either \"speed\" or \"temperature\"");
                }
            };
            Some(monitor::FanControlConfig {
                mode,
                min_fans: fan_config.min_fans,
            })
        } else {
            None
        };

        monitor::Config {
            temp_config: monitor_temp_config,
            fan_config: monitor_fan_config,
        }
    }

    pub fn parse(config_path: &str) -> Self {
        // Parse config file - either user specified or the default one
        let mut generic_config = bosminer_config::parse(config_path);

        // Parse pools
        // Don't worry if is this section missing, maybe there are some pools on command line
        let pools = generic_config.pools.take().unwrap_or_else(|| Vec::new());

        // parse user input to fail fast when it is incorrect
        let clients = pools
            .into_iter()
            .map(|pool| {
                bosminer_config::client::parse(pool.url.clone(), pool.user.clone())
                    .expect("Server parameters")
            })
            .collect();

        let mut configuration = generic_config
            .backend_config
            .try_into::<Self>()
            .expect("failed to interpret config file");

        configuration.clients = clients;

        configuration
    }
}

impl hal::BackendConfig for Backend {
    #[inline]
    fn midstate_count(&self) -> usize {
        if self.asic_boost {
            DEFAULT_MIDSTATE_COUNT
        } else {
            1
        }
    }

    fn clients(&mut self) -> Vec<bosminer_config::client::Descriptor> {
        self.clients.drain(..).collect()
    }
}
