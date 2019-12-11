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

use bosminer::clap;

use crate::bm1387::MidstateCount;
use crate::fan;
use crate::monitor;
use crate::power;
use crate::FrequencySettings;

use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

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

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct TempConfig {
    dangerous_temp: f32,
    hot_temp: f32,
}

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct FanConfig {
    temperature: Option<f32>,
    speed: Option<usize>,
    min_fans: usize,
}

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct ChainConfig {
    frequency: Option<f32>,
    voltage: Option<f32>,
}

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    frequency: f32,
    voltage: f32,
    asic_boost: bool,
    temperature: Option<TempConfig>,
    fans: Option<FanConfig>,
    chain: Option<HashMap<String, ChainConfig>>,
}

pub struct ResolvedChainConfig {
    pub midstate_count: MidstateCount,
    pub frequency: FrequencySettings,
    pub voltage: power::Voltage,
}

impl Configuration {
    pub fn add_args<'a, 'b>(app: clap::App<'a, 'b>) -> clap::App<'a, 'b> {
        app.arg(
            clap::Arg::with_name("disable-asic-boost")
                .long("disable-asic-boost")
                .help("Disable ASIC boost (use just one midstate)")
                .required(false),
        )
        .arg(
            clap::Arg::with_name("frequency")
                .long("frequency")
                .help("Set chip frequency (in MHz)")
                .required(false)
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("voltage")
                .long("voltage")
                .help("Set chip voltage (in volts)")
                .required(false)
                .takes_value(true),
        )
    }

    pub fn midstate_count(&self) -> usize {
        if self.asic_boost {
            DEFAULT_MIDSTATE_COUNT
        } else {
            1
        }
    }

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

    pub fn parse(matches: &clap::ArgMatches, backend_config: ::config::Value) -> Self {
        let mut configuration = backend_config
            .try_into::<Self>()
            .expect("failed to interpret config file");

        // Set just 1 midstate if user requested disabling asicboost
        if matches.is_present("disable-asic-boost") {
            configuration.asic_boost = false;
        }
        if let Some(value) = matches.value_of("frequency") {
            configuration.frequency = value.parse::<f32>().expect("not a float number");
        }
        if let Some(value) = matches.value_of("voltage") {
            configuration.voltage = value.parse::<f32>().expect("not a float number");
        }
        configuration
    }
}

// write some tests please somebody
