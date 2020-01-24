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

use ii_logging::macros::*;

pub mod api;
mod support;

use crate::bm1387::MidstateCount;
use crate::error;
use crate::fan;
use crate::monitor;
use crate::power;
use crate::FrequencySettings;

use support::OptionDefault;

use bosminer::hal::{self, BackendConfig as _};

use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;
use std::time::Duration;

/// Expected configuration version
const FORMAT_VERSION: &'static str = "beta";

/// Expected configuration model
const FORMAT_MODEL: &'static str = "Antminer S9";

/// Override the default drain channel size as miner tends to burst messages into the logger
pub const ASYNC_LOGGER_DRAIN_CHANNEL_SIZE: usize = 4096;

/// Location of default config
/// TODO: Maybe don't add `.toml` prefix so we could use even JSON
pub const DEFAULT_CONFIG_PATH: &'static str = "/etc/bosminer.toml";

/// Default number of midstates when AsicBoost is enabled
pub const ASIC_BOOST_MIDSTATE_COUNT: usize = 4;

/// Default number of midstates
pub const DEFAULT_ASIC_BOOST: bool = true;

/// Default PLL frequency for clocking the chips in MHz
pub const DEFAULT_FREQUENCY: f32 = 650.0;

/// Default voltage
pub const DEFAULT_VOLTAGE: f32 = 8.8;

/// Default temperature control mode
pub const DEFAULT_TEMP_CONTROL_MODE: TempControlMode = TempControlMode::Auto;

/// Default temperatures for temperature control
pub const DEFAULT_TARGET_TEMP: f32 = 75.0;
pub const DEFAULT_HOT_TEMP: f32 = 95.0;
pub const DEFAULT_DANGEROUS_TEMP: f32 = 105.0;

/// Default fan speed for manual target speed
pub const DEFAULT_FAN_SPEED: usize = 100;

/// Default minimal running fans for monitoring
pub const DEFAULT_MIN_FANS: usize = 1;

/// Index of hashboard that is to be instantiated
pub const S9_HASHBOARD_INDEX: usize = 8;

/// Range of hash chain index
pub const HASH_CHAIN_INDEX_MIN: usize = 1;
pub const HASH_CHAIN_INDEX_MAX: usize = 9;

/// Range of PLL frequency for clocking the chips in MHz
pub const FREQUENCY_MIN: f32 = 200.0;
pub const FREQUENCY_MAX: f32 = 900.0;

/// Range of hash chain voltage
pub const VOLTAGE_MIN: f32 = 7.95;
pub const VOLTAGE_MAX: f32 = 9.4;

/// Range of monitored temperature
pub const TEMPERATURE_MIN: f32 = 0.0;
pub const TEMPERATURE_MAX: f32 = 200.0;

/// Range of monitored temperature
pub const FAN_SPEED_MIN: usize = 0;
pub const FAN_SPEED_MAX: usize = 100;

/// Range of possible fans
pub const FANS_MIN: usize = 0;
pub const FANS_MAX: usize = 4;

/// Default ASIC difficulty
pub const ASIC_DIFFICULTY: usize = 64;

/// Default hashrate interval used for statistics in seconds
pub const DEFAULT_HASHRATE_INTERVAL: Duration = Duration::from_secs(60);

/// Maximum time it takes to compute one job under normal circumstances
pub const JOB_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ResolvedChainConfig {
    pub midstate_count: MidstateCount,
    pub frequency: FrequencySettings,
    pub voltage: power::Voltage,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TempControlMode {
    Auto,
    Manual,
    Disabled,
}

impl std::string::ToString for TempControlMode {
    fn to_string(&self) -> String {
        match self {
            Self::Auto => "auto".to_string(),
            Self::Manual => "manual".to_string(),
            Self::Disabled => "disabled".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Format {
    version: String,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    generator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<u32>,
}

impl Default for Format {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION.to_string(),
            model: FORMAT_MODEL.to_string(),
            generator: None,
            timestamp: None,
        }
    }
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct HashChainGlobal {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asic_boost: Option<bool>,
    #[serde(flatten)]
    pub overridable: Option<HashChain>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct HashChain {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voltage: Option<f32>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct TempControl {
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<TempControlMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_temp: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hot_temp: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dangerous_temp: Option<f32>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct FanControl {
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_fans: Option<usize>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Backend {
    format: Format,
    // TODO: merge pools and clients
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_chain_global: Option<HashChainGlobal>,
    #[serde(rename = "hash_chain")]
    #[serde(skip_serializing_if = "Option::is_none")]
    hash_chains: Option<BTreeMap<String, HashChain>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temp_control: Option<TempControl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fan_control: Option<FanControl>,
    #[serde(rename = "pool")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pools: Option<Vec<bosminer_config::PoolConfig>>,
    #[serde(skip)]
    pub clients: Vec<bosminer_config::client::Descriptor>,
}

impl Backend {
    pub fn resolve_chain_config(&self, hash_chain_idx: usize) -> ResolvedChainConfig {
        // Take global hash chain configuration or default value
        let overridable = self
            .hash_chain_global
            .as_ref()
            .and_then(|v| v.overridable.as_ref());
        let mut frequency = OptionDefault::new(
            overridable.as_ref().and_then(|v| v.frequency),
            DEFAULT_FREQUENCY,
        );
        let mut voltage = OptionDefault::new(
            overridable.as_ref().and_then(|v| v.voltage),
            DEFAULT_VOLTAGE,
        );

        // If there's a per-chain override then apply it
        if let Some(hash_chain) = self
            .hash_chains
            .as_ref()
            .and_then(|m| m.get(&hash_chain_idx.to_string()))
        {
            frequency = hash_chain
                .frequency
                .map(|v| OptionDefault::Some(v))
                .unwrap_or(frequency);
            voltage = hash_chain
                .voltage
                .map(|v| OptionDefault::Some(v))
                .unwrap_or(voltage);
        }

        // Computed s9-specific values
        ResolvedChainConfig {
            midstate_count: MidstateCount::new(self.midstate_count()),
            frequency: FrequencySettings::from_frequency((*frequency * 1_000_000.0) as usize),
            // TODO: handle config errors
            voltage: power::Voltage::from_volts(*voltage).expect("bad voltage requested"),
        }
    }

    pub fn resolve_monitor_config(&self) -> monitor::Config {
        // Get temperature control settings
        let mode = OptionDefault::new(
            self.temp_control.as_ref().and_then(|v| v.mode),
            DEFAULT_TEMP_CONTROL_MODE,
        );
        let target_temp = OptionDefault::new(
            self.temp_control.as_ref().and_then(|v| v.target_temp),
            DEFAULT_TARGET_TEMP,
        );
        let hot_temp = OptionDefault::new(
            self.temp_control.as_ref().and_then(|v| v.hot_temp),
            DEFAULT_HOT_TEMP,
        );
        let dangerous_temp = OptionDefault::new(
            self.temp_control.as_ref().and_then(|v| v.dangerous_temp),
            DEFAULT_DANGEROUS_TEMP,
        );

        // Get fan control settings
        let fan_speed = OptionDefault::new(
            self.fan_control.as_ref().and_then(|v| v.speed),
            DEFAULT_FAN_SPEED,
        );
        let min_fans = OptionDefault::new(
            self.fan_control.as_ref().and_then(|v| v.min_fans),
            DEFAULT_MIN_FANS,
        );

        let temp_config;
        let fan_config;

        // Configure temperature controller
        match *mode {
            TempControlMode::Auto | TempControlMode::Manual => {
                temp_config = Some(monitor::TempControlConfig {
                    dangerous_temp: *dangerous_temp,
                    hot_temp: *hot_temp,
                });
            }
            TempControlMode::Disabled => {
                temp_config = None;
                // do sanity checks
                if hot_temp.is_some() {
                    warn!(
                        "Unused 'hot_temp' ({}) because 'disable' mode is set",
                        *hot_temp
                    );
                }
                if dangerous_temp.is_some() {
                    warn!(
                        "Unused 'dangerous_temp' ({}) because 'disable' mode is set",
                        *hot_temp
                    );
                }
            }
        };

        // Configure fan controller
        match *mode {
            TempControlMode::Auto => {
                fan_config = Some(monitor::FanControlConfig {
                    mode: monitor::FanControlMode::TargetTemperature(*target_temp),
                    min_fans: *min_fans,
                });
                // do sanity checks
                if fan_speed.is_some() {
                    warn!(
                        "Unused fan 'speed' ({}) because 'auto' mode is set",
                        *fan_speed
                    );
                }
            }
            TempControlMode::Manual | TempControlMode::Disabled => {
                fan_config = if fan_speed.eq_some(&0) && min_fans.eq_some(&0) {
                    // completely disable fan controller when all settings are set to 0
                    None
                } else {
                    Some(monitor::FanControlConfig {
                        mode: monitor::FanControlMode::FixedSpeed(fan::Speed::new(*fan_speed)),
                        min_fans: *min_fans,
                    })
                };
                // do sanity checks
                if target_temp.is_some() {
                    warn!(
                        "Unused 'target_temp' ({}) because 'auto' mode is not set",
                        *fan_speed
                    );
                }
            }
        };

        monitor::Config {
            temp_config,
            fan_config,
        }
    }

    pub fn parse(config_path: &str) -> error::Result<Self> {
        // Parse config file - either user specified or the default one
        let mut backend_config: Self = bosminer_config::parse(config_path)?;

        // Check compatibility of configuration format
        if backend_config.format.model != FORMAT_MODEL {
            Err(format!(
                "incompatible format model '{}'",
                backend_config.format.model
            ))?;
        }
        // TODO: allow backward compatibility
        if backend_config.format.version != FORMAT_VERSION {
            Err(format!(
                "incompatible format version '{}'",
                backend_config.format.version
            ))?;
        }

        // Check if all hash chain keys have meaningful name
        if let Some(hash_chains) = &backend_config.hash_chains {
            for idx in hash_chains.keys() {
                let _ = idx
                    .parse::<usize>()
                    .map_err(|_| format!("hash chain index '{}' is not number", idx))
                    .and_then(|idx| {
                        if (HASH_CHAIN_INDEX_MIN..=HASH_CHAIN_INDEX_MAX).contains(&idx) {
                            Ok(idx)
                        } else {
                            Err(format!(
                                "hash chain index '{}' is out of range '{}..{}'",
                                idx, HASH_CHAIN_INDEX_MIN, HASH_CHAIN_INDEX_MAX
                            ))
                        }
                    })?;
            }
        }

        // Parse pools
        // Don't worry if is this section missing, maybe there are some pools on command line
        if let Some(pools) = backend_config.pools.as_ref() {
            // parse user input to fail fast when it is incorrect
            backend_config.clients = pools
                .into_iter()
                .map(|pool| {
                    // TODO: do not panic!
                    bosminer_config::client::parse(pool.url.clone(), pool.user.clone())
                        .expect("Server parameters")
                })
                .collect();
        }

        Ok(backend_config)
    }
}

impl hal::BackendConfig for Backend {
    #[inline]
    fn midstate_count(&self) -> usize {
        if self
            .hash_chain_global
            .as_ref()
            .and_then(|v| v.asic_boost)
            .unwrap_or(DEFAULT_ASIC_BOOST)
        {
            ASIC_BOOST_MIDSTATE_COUNT
        } else {
            1
        }
    }

    fn clients(&mut self) -> Vec<bosminer_config::client::Descriptor> {
        self.clients.drain(..).collect()
    }
}
