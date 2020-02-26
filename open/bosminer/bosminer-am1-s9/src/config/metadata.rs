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

//! Temporary location of config metadata

use super::*;

use bosminer_config::CLIENT_URL_JAVA_SCRIPT_REGEX;

use serde_json::{self, json};

pub fn for_backend() -> serde_json::Value {
    json!([
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
            "group",
            {
                "type": "array",
                "label": "Groups",
                "sortable": true,
                "item": {
                    "type": "object",
                    "fields": [
                        [
                            "name",
                            {
                                "type": "string",
                                "label": "Name",
                                "min_length": 1,
                                "span": 6
                            }
                        ],
                        [
                            "quota",
                            {
                                "type": "number",
                                "label": "Quota",
                                "default": 1,
                                "span": 3
                            }
                        ],
                        [
                            "fixed_share_ratio",
                            {
                                "type": "number",
                                "label": "Fixed Share Ratio",
                                "min": 0.0,
                                "max": 1.0,
                                "step": 0.01,
                                "float": true,
                                "default": null,
                                "span": 3
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
                                            "enabled",
                                            {
                                                "type": "bool",
                                                "label": "Enabled",
                                                "default": DEFAULT_POOL_ENABLED,
                                                "span": 1
                                            }
                                        ],
                                        [
                                            "url",
                                            {
                                                "type": "url",
                                                "label": "URL",
                                                "min_length": 1,
                                                "match": CLIENT_URL_JAVA_SCRIPT_REGEX,
                                                "span": 5
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
                                                "span": 2
                                            }
                                        ]
                                    ]
                                }
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
                            "disabled": ["$neq", ["$get", "temp_control", "mode"], "auto"],
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
                            "disabled": ["$eq", ["$get", "temp_control", "mode"], "disabled"],
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
                            "disabled": ["$eq", ["$get", "temp_control", "mode"], "disabled"],
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
                            "disabled": ["$eq", ["$get", "temp_control", "mode"], "auto"]
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
    ])
}
