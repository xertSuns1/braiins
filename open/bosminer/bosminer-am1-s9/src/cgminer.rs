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

use ii_cgminer_api::command::{DEVDETAILS, FANS, TEMPCTRL, TEMPS};
use ii_cgminer_api::{command, commands, response};

use serde::Serialize;

use std::sync::Arc;

use crate::monitor;
use crate::sensor;

use futures::lock::Mutex;
use ii_async_compat::futures;

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
#[repr(u32)]
pub enum StatusCode {
    NotReady = 1,
}

impl From<StatusCode> for u32 {
    fn from(code: StatusCode) -> Self {
        code as u32
    }
}

pub enum ErrorCode {
    NotReady,
}

impl From<ErrorCode> for response::Error {
    fn from(code: ErrorCode) -> Self {
        let (code, msg) = match code {
            ErrorCode::NotReady => (StatusCode::NotReady, "Not ready".to_string()),
        };

        Self::from_custom_error(code, msg)
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct DevDetailInfo {
    #[serde(rename = "Voltage")]
    pub voltage: f64,
    #[serde(rename = "Frequency")]
    pub frequency: u32,
    #[serde(rename = "Chips")]
    pub chips: u32,
    #[serde(rename = "Cores")]
    pub cores: u32,
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct TempInfo {
    #[serde(rename = "Board")]
    pub board: f64,
    #[serde(rename = "Chip")]
    pub chip: f64,
}

pub struct Handler {
    managers: Vec<Arc<Mutex<crate::HashChainManager>>>,
    monitor: Arc<monitor::Monitor>,
}

impl Handler {
    pub fn new(
        managers: Vec<Arc<Mutex<crate::HashChainManager>>>,
        monitor: Arc<monitor::Monitor>,
    ) -> Self {
        Self { managers, monitor }
    }

    fn get_monitor_status(&self) -> command::Result<monitor::Status> {
        match self.monitor.status_receiver.borrow().clone() {
            Some(status) => Ok(status),
            None => Err(ErrorCode::NotReady.into()),
        }
    }

    async fn handle_dev_details(&self) -> command::Result<response::DevDetails<DevDetailInfo>> {
        let mut list = vec![];
        for manager in self.managers.iter() {
            let manager = manager.lock().await;
            let runtime = manager.runtime.as_ref();
            let chip_count = match runtime {
                Some(runtime) => runtime.hash_chain.chip_count,
                None => 0,
            };
            list.push(response::DevDetail {
                idx: list.len() as i32,
                name: "".to_string(),
                id: manager.node.hashboard_idx as i32,
                driver: "".to_string(),
                kernel: "".to_string(),
                model: "".to_string(),
                device_path: "".to_string(),
                info: DevDetailInfo {
                    voltage: manager.params.voltage.as_volts() as f64,
                    frequency: manager.params.frequency.max() as u32,
                    chips: chip_count as u32,
                    cores: (chip_count * crate::bm1387::NUM_CORES_ON_CHIP) as u32,
                },
            });
        }

        Ok(response::DevDetails { list })
    }

    async fn handle_temp_ctrl(&self) -> command::Result<response::ext::TempCtrl> {
        let config = self.get_monitor_status()?.config;
        let (hot, dangerous) = match config.temp_config {
            Some(config) => (config.hot_temp, config.dangerous_temp),
            None => (0.0, 0.0),
        };
        let (mode, target_temp, target_fan, _min_fans) = match config.fan_config {
            Some(config) => match config.mode {
                monitor::FanControlMode::FixedSpeed(speed) => {
                    ("fixed fan speed", 0.0, speed.to_pwm(), config.min_fans)
                }
                monitor::FanControlMode::TargetTemperature(temp) => {
                    ("target temperature", temp, 0, config.min_fans)
                }
            },
            None => ("disabled", 0.0, 0, 0),
        };
        Ok(response::ext::TempCtrl {
            mode: mode.to_string(),
            target: (target_temp as f64) + (target_fan as f64),
            hot: hot as f64,
            dangerous: dangerous as f64,
        })
    }

    async fn handle_temps(&self) -> command::Result<response::ext::Temps<TempInfo>> {
        let mut list = vec![];
        for manager in self.managers.iter() {
            let manager = manager.lock().await;
            if let Some(runtime) = manager.runtime.as_ref() {
                if let Some(sensor::Temperature { local, remote }) = runtime.current_temperature() {
                    list.push(response::ext::Temp {
                        idx: list.len() as i32,
                        id: manager.node.hashboard_idx as i32,
                        info: TempInfo {
                            board: Option::from(local).unwrap_or(0.0) as f64,
                            chip: Option::from(remote).unwrap_or(0.0) as f64,
                        },
                    });
                }
            }
        }
        Ok(response::ext::Temps { list: list })
    }

    async fn handle_fans(&self) -> command::Result<response::ext::Fans> {
        let status = self.get_monitor_status()?;
        let speed = status.fan_speed.map(|speed| speed.to_pwm()).unwrap_or(0);
        Ok(response::ext::Fans {
            list: status
                .fan_feedback
                .rpm
                .iter()
                .enumerate()
                .map(|(id, rpm)| response::ext::Fan {
                    idx: id as i32,
                    id: id as i32,
                    speed: speed as u32,
                    rpm: *rpm as u32,
                })
                .collect(),
        })
    }
}

pub fn create_custom_commands(
    managers: Vec<Arc<Mutex<crate::HashChainManager>>>,
    monitor: Arc<monitor::Monitor>,
) -> Option<command::Map> {
    let handler = Arc::new(Handler::new(managers, monitor));

    let custom_commands = commands![
        (DEVDETAILS: ParameterLess -> handler.handle_dev_details),
        (TEMPCTRL: ParameterLess -> handler.handle_temp_ctrl),
        (TEMPS: ParameterLess -> handler.handle_temps),
        (FANS: ParameterLess -> handler.handle_fans)
    ];

    Some(custom_commands)
}
