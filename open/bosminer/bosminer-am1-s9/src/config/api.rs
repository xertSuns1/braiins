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

//! This module handles configuration commands needed for configuration backend API

use super::*;

use serde::{Deserialize, Serialize};
use serde_json::{self, json};
use serde_repr::*;

use std::io::{self, Write};
use std::path::Path;
use std::time::SystemTime;

// TODO: move it to shared crate
pub struct UnixTime;

impl UnixTime {
    fn now() -> u32 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as u32)
            .unwrap_or(0)
    }
}

fn generator_string() -> String {
    format!("bosminer {}", bosminer::version::STRING.clone())
}

#[derive(Serialize_repr, Eq, PartialEq, Copy, Clone, Debug)]
#[repr(u32)]
pub enum StatusCode {
    Success = 0,

    // error codes
    SystemError = 1,
    MissingFile = 2,
    InvalidFormat = 3,
    IncompatibleFormatVersion = 4,
}

#[derive(Serialize, Clone, Debug)]
struct Status {
    code: StatusCode,
    message: Option<String>,
    generator: String,
    timestamp: u32,
}

impl Status {
    fn new<T: Into<Option<String>>>(code: StatusCode, message: T) -> Self {
        Self {
            code,
            message: message.into(),
            generator: generator_string(),
            timestamp: UnixTime::now(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
struct MetadataResponse {
    pub status: Status,
    pub data: serde_json::Value,
}

#[derive(Serialize, Clone, Debug)]
struct DataResponse {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Backend>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
struct SaveRequest {
    pub data: serde_json::Value,
}

#[derive(Serialize, Clone, Debug)]
struct SaveSuccess {
    pub path: String,
    pub format: Format,
}

#[derive(Serialize, Clone, Debug)]
struct SaveResponse {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<SaveSuccess>,
}

pub struct Handler<'a> {
    config_path: &'a str,
}

impl<'a> Handler<'a> {
    pub fn new(config_path: &'a str) -> Self {
        Self { config_path }
    }

    fn send_response<T>(self, response: T)
    where
        T: Serialize,
    {
        serde_json::to_writer(io::stdout(), &response).expect("BUG: cannot serialize response");
    }

    pub fn handle_metadata(self) {
        let metadata = json!([
            [
                "format",
                {
                    "type": "object",
                    "label": "Configuration File Details",
                    "fields": [
                        [
                            "version",
                            {
                                "type": "string",
                                "label": "Version",
                                "span": 6
                            }
                        ],
                        [
                            "model",
                            {
                                "type": "string",
                                "label": "Model",
                                "span": 6
                            }
                        ],
                        [
                            "generator",
                            {
                                "type": "string",
                                "label": "Generator",
                                "default": null,
                                "span": 6
                            }
                        ],
                        [
                            "timestamp",
                            {
                                "type": "time",
                                "label": "Timestamp",
                                "default": null,
                                "span": 6
                            }
                        ]
                    ],
                    "readonly": true
                }
            ],
            [
                "pool",
                {
                    "type": "array",
                    "label": "List of Pools",
                    "sortable": true,
                    "item": {
                        "type": "object",
                        "fields": [
                            [
                                "url",
                                {
                                    "type": "url",
                                    "label": "URL",
                                    "min_length": 1,
                                    "span": 4
                                }
                            ],
                            [
                                "user",
                                {
                                    "type": "string",
                                    "label": "User",
                                    "min_length": 1,
                                    "span": 4
                                }
                            ],
                            [
                                "password",
                                {
                                    "type": "password",
                                    "label": "Password",
                                    "default": null,
                                    "span": 4
                                }
                            ]
                        ]
                    }
                }
            ],
            [
                "hash_chain_global",
                {
                    "type": "object",
                    "label": "Global Hash Chain Settings",
                    "fields": [
                        [
                            "asic_boost",
                            {
                                "type": "bool",
                                "label": "AsicBoost",
                                "default": DEFAULT_ASIC_BOOST
                            }
                        ],
                        [
                            "frequency",
                            {
                                "type": "number",
                                "label": "Frequency",
                                "unit": "MHz",
                                "min": FREQUENCY_MHZ_MIN,
                                "max": FREQUENCY_MHZ_MAX,
                                "float": true,
                                "default": DEFAULT_FREQUENCY_MHZ
                            }
                        ],
                        [
                            "voltage",
                            {
                                "type": "number",
                                "label": "Voltage",
                                "unit": "V",
                                "min": VOLTAGE_V_MIN,
                                "max": VOLTAGE_V_MAX,
                                "float": true,
                                "default": DEFAULT_VOLTAGE_V
                            }
                        ]
                    ]
                }
            ],
            [
                "hash_chain",
                {
                    "type": "dict",
                    "label": "Override Global Hash Chain Settings",
                    "key": {
                        "min": HASH_CHAIN_INDEX_MIN,
                        "max": HASH_CHAIN_INDEX_MAX
                    },
                    "value": {
                        "type": "object",
                        "fields": [
                            [
                                "frequency",
                                {
                                    "type": "number",
                                    "label": "Frequency",
                                    "unit": "MHz",
                                    "min": FREQUENCY_MHZ_MIN,
                                    "max": FREQUENCY_MHZ_MAX,
                                    "float": true,
                                    "default": ["$get", "hash_chain_global", "frequency"],
                                    "span": 6
                                }
                            ],
                            [
                                "voltage",
                                {
                                    "type": "number",
                                    "label": "Voltage",
                                    "unit": "V",
                                    "min": VOLTAGE_V_MIN,
                                    "max": VOLTAGE_V_MAX,
                                    "float": true,
                                    "default": ["$get", "hash_chain_global", "voltage"],
                                    "span": 6
                                }
                            ]
                        ]
                    }
                }
            ],
            [
                "temp_control",
                {
                    "type": "object",
                    "label": "Temperature Control",
                    "fields": [
                        [
                            "mode",
                            {
                                "type": "enum",
                                "label": "Mode",
                                "values": [
                                    {
                                        "key": TempControlMode::Auto.to_string(),
                                        "label": "Automatic"
                                    },
                                    {
                                        "key": TempControlMode::Manual.to_string(),
                                        "label": "Manual",
                                        "alert": "Warning ..."
                                    },
                                    {
                                        "key": TempControlMode::Disabled.to_string(),
                                        "label": "Disabled",
                                        "alert": "Warning ..."
                                    }
                                ],
                                "default": TempControlMode::Auto.to_string()
                            }
                        ],
                        [
                            "target_temp",
                            {
                                "type": "number",
                                "label": "Target Temperature",
                                "unit": "°C",
                                "min": TEMPERATURE_C_MIN,
                                "max": TEMPERATURE_C_MAX,
                                "float": true,
                                "default": DEFAULT_TARGET_TEMP_C,
                                "readonly": ["$neq", ["$get", "temp_control", "mode"], "auto"],
                                "span": 4
                            }
                        ],
                        [
                            "hot_temp",
                            {
                                "type": "number",
                                "label": "Hot Temperature",
                                "unit": "°C",
                                "min": TEMPERATURE_C_MIN,
                                "max": TEMPERATURE_C_MAX,
                                "float": true,
                                "default": DEFAULT_HOT_TEMP_C,
                                "readonly": ["$eq", ["$get", "temp_control", "mode"], "disabled"],
                                "span": 4
                            }
                        ],
                        [
                            "dangerous_temp",
                            {
                                "type": "number",
                                "label": "Dangerous Temperature",
                                "unit": "°C",
                                "min": TEMPERATURE_C_MIN,
                                "max": TEMPERATURE_C_MAX,
                                "float": true,
                                "default": DEFAULT_DANGEROUS_TEMP_C,
                                "readonly": ["$eq", ["$get", "temp_control", "mode"], "disabled"],
                                "span": 4
                            }
                        ]
                    ]
                }
            ],
            [
                "fan_control",
                {
                    "type": "object",
                    "label": "Fan Control",
                    "fields": [
                        [
                            "speed",
                            {
                                "type": "number",
                                "label": "Speed",
                                "unit": "%",
                                "min": FAN_SPEED_MIN,
                                "max": FAN_SPEED_MAX,
                                "default": DEFAULT_FAN_SPEED,
                                "readonly": ["$eq", ["$get", "temp_control", "mode"], "auto"]
                            }
                        ],
                        [
                            "min_fans",
                            {
                                "type": "number",
                                "label": "Minimum Running Fans",
                                "min": FANS_MIN,
                                "max": FANS_MAX,
                                "default": DEFAULT_MIN_FANS
                            }
                        ]
                    ]
                }
            ]
        ]);

        let response = MetadataResponse {
            status: Status::new(StatusCode::Success, None),
            data: metadata,
        };

        self.send_response(response);
    }

    pub fn handle_data(self) {
        let response = match Backend::parse(self.config_path) {
            // TODO: Improve error handling
            Err(e) => DataResponse {
                status: Status::new(StatusCode::InvalidFormat, format!("{}", e)),
                data: None,
            },
            Ok(backend_config) => DataResponse {
                status: Status::new(StatusCode::Success, None),
                data: Some(backend_config),
            },
        };

        self.send_response(response);
    }

    pub fn handle_save(self) {
        let mut request: SaveRequest =
            serde_json::from_reader(io::stdin()).expect("TODO: deserialize SaveRequest");

        let config_format = Format {
            generator: generator_string().into(),
            timestamp: UnixTime::now().into(),
            ..Default::default()
        };

        let json_format =
            serde_json::to_value(config_format).expect("BUG: cannot serialize Format");
        request
            .data
            .as_object_mut()
            .expect("TODO: invalid data type")
            .insert("format".to_string(), json_format);

        let backend_config: Backend =
            serde_json::from_value(request.data).expect("TODO: deserialize Backend");

        let config_path = Path::new(self.config_path);
        let config_dir = config_path.parent().expect("TODO: path.parent");
        assert!(config_dir.exists());

        let mut file =
            tempfile::NamedTempFile::new_in(config_dir).expect("TODO: NamedTempFile::new");

        file.write_all(
            toml::to_string_pretty(&backend_config)
                .expect("TODO: toml::to_string_pretty")
                .as_bytes(),
        )
        .expect("TODO: file.write_all");

        file.persist(config_path).expect("TODO: file.persist");

        let response = SaveResponse {
            status: Status::new(StatusCode::Success, None),
            data: Some(SaveSuccess {
                path: config_path
                    .canonicalize()
                    .expect("TODO: path.canonicalize")
                    .into_os_string()
                    .into_string()
                    .expect("TODO: into_os_string"),
                format: backend_config.format,
            }),
        };

        self.send_response(response);
    }
}
