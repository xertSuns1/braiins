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

//! Tests for the CGMiner API module

mod handler;
mod utils;

use crate::command;
use crate::commands;
use crate::response;

use utils::{assert_json_eq, codec_roundtrip};

use ii_async_compat::tokio;

use serde::Serialize;
use serde_json as json;

use std::sync::Arc;

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
#[repr(u32)]
pub enum CustomStatusCode {
    CustomCommandOne = 1,
    CustomCommandTwo = 2,

    // custom error codes
    MissingParameter = 10,
}

impl From<CustomStatusCode> for u32 {
    fn from(code: CustomStatusCode) -> Self {
        code as u32
    }
}

pub enum CustomErrorCode {
    MissingParameter(String),
}

impl From<CustomErrorCode> for response::Error {
    fn from(code: CustomErrorCode) -> Self {
        let (code, msg) = match code {
            CustomErrorCode::MissingParameter(name) => (
                CustomStatusCode::MissingParameter,
                format!("Missing parameter '{}'", name),
            ),
        };

        Self::from_custom_error(code, msg)
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct CustomCommandOne {
    #[serde(rename = "Attribute")]
    pub attribute: String,
}

impl From<CustomCommandOne> for response::Dispatch {
    fn from(custom_command: CustomCommandOne) -> Self {
        response::Dispatch::from_custom_success(
            CustomStatusCode::CustomCommandOne,
            format!("{} custom command {}", crate::SIGNATURE_TAG, 1),
            Some(response::Body {
                name: "CUSTOM_COMMAND_ONE",
                list: vec![custom_command],
            }),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct CustomCommandTwo {
    #[serde(rename = "Value")]
    pub value: u32,
}

impl From<CustomCommandTwo> for response::Dispatch {
    fn from(custom_command: CustomCommandTwo) -> Self {
        response::Dispatch::from_custom_success(
            CustomStatusCode::CustomCommandTwo,
            format!(
                "{} custom command {} with parameter",
                crate::SIGNATURE_TAG,
                2
            ),
            Some(response::Body {
                name: "CUSTOM_COMMAND_TWO",
                list: vec![custom_command],
            }),
        )
    }
}

struct TestCustomHandler;

impl TestCustomHandler {
    async fn handle_command_one(&self) -> command::Result<CustomCommandOne> {
        Ok(CustomCommandOne {
            attribute: "value".to_string(),
        })
    }

    async fn handle_command_two(
        &self,
        parameter: Option<&json::Value>,
    ) -> command::Result<CustomCommandTwo> {
        parameter
            .ok_or_else(|| CustomErrorCode::MissingParameter("value".to_string()).into())
            .map(|value| {
                let value = value.as_u64().unwrap() as u32;
                CustomCommandTwo { value }
            })
    }
}

#[tokio::test]
async fn test_single() {
    let command: json::Value = json::json!({
        "command": "version"
    });
    let response = codec_roundtrip(command, None).await;
    let expected = json::json!({
        "STATUS": [{
            "STATUS": "S",
            "When": 0,
            "Code": 22,
            "Msg": "TestMiner versions",
            "Description": "TestMiner v1.0",
        }],
        "VERSION": [{
            "API": "3.7",
            "TestMiner": "v1.0"
        }],
        "id": 1
    });

    assert_json_eq(&response, &expected);
}

#[tokio::test]
async fn test_multiple() {
    let command: json::Value = json::json!({
        "command": "version+config"
    });
    let response = codec_roundtrip(command, None).await;
    let expected = json::json!({
        "config": [{
            "STATUS": [{
                "Code": 33,
                "Description": "TestMiner v1.0",
                "Msg": "TestMiner config",
                "STATUS": "S",
                "When": 0
            }],
            "CONFIG": [{
                "ASC Count": 0,
                "Device Code": "",
                "Hotplug": "None",
                "Log Interval": 0,
                "OS": "Braiins OS",
                "PGA Count": 0,
                "Pool Count": 0,
                "Strategy": "Failover"
            }],
            "id": 1
        }],
        "version": [{
            "STATUS": [{
                "Code": 22,
                "Description": "TestMiner v1.0",
                "Msg": "TestMiner versions",
                "STATUS": "S",
                "When": 0
            }],
            "VERSION": [{
                "API": "3.7",
                "TestMiner": "v1.0"
            }],
            "id": 1
        }],
        "id": 1,
    });

    assert_json_eq(&response, &expected);
}

#[tokio::test]
async fn test_single_custom_command() {
    let handler = Arc::new(TestCustomHandler);

    const CUSTOM_COMMAND: &str = "custom_command";
    let custom_commands = commands![
        (CUSTOM_COMMAND: ParameterLess -> handler.handle_command_one)
    ];

    let command: json::Value = json::json!({ "command": CUSTOM_COMMAND });

    let response = codec_roundtrip(command, custom_commands).await;
    let expected = json::json!({
        "STATUS": [{
            "STATUS": "S",
            "When": 0,
            "Code": 301,
            "Msg": "TestMiner custom command 1",
            "Description": "TestMiner v1.0",
        }],
        "CUSTOM_COMMAND_ONE": [{
            "Attribute": "value",
        }],
        "id": 1
    });

    assert_json_eq(&response, &expected);
}

#[tokio::test]
async fn test_single_custom_command_with_parameter() {
    let handler = Arc::new(TestCustomHandler);

    const CUSTOM_COMMAND: &str = "custom_command";
    let custom_commands = commands![
        (CUSTOM_COMMAND: Parameter(None) -> handler.handle_command_two)
    ];

    let command: json::Value = json::json!({
        "command": CUSTOM_COMMAND,
        "parameter": 42
    });

    let response = codec_roundtrip(command, custom_commands).await;
    let expected = json::json!({
        "STATUS": [{
            "STATUS": "S",
            "When": 0,
            "Code": 302,
            "Msg": "TestMiner custom command 2 with parameter",
            "Description": "TestMiner v1.0",
        }],
        "CUSTOM_COMMAND_TWO": [{
            "Value": 42,
        }],
        "id": 1
    });

    assert_json_eq(&response, &expected);
}

#[tokio::test]
async fn test_single_custom_command_error() {
    let handler = Arc::new(TestCustomHandler);

    const CUSTOM_COMMAND: &str = "custom_command";
    let custom_commands = commands![
        (CUSTOM_COMMAND: Parameter(None) -> handler.handle_command_two)
    ];

    let command: json::Value = json::json!({ "command": CUSTOM_COMMAND });

    let response = codec_roundtrip(command, custom_commands).await;
    let expected = json::json!({
        "STATUS": [{
            "STATUS": "E",
            "When": 0,
            "Code": 310,
            "Msg": "Missing parameter 'value'",
            "Description": "TestMiner v1.0",
        }],
        "id": 1
    });

    assert_json_eq(&response, &expected);
}

#[tokio::test]
async fn test_multiple_custom_commands() {
    let handler = Arc::new(TestCustomHandler);

    const CUSTOM_COMMAND_ONE: &str = "custom_command_one";
    const CUSTOM_COMMAND_TWO: &str = "custom_command_two";

    let custom_commands = commands![
        (CUSTOM_COMMAND_ONE: ParameterLess -> handler.handle_command_one),
        (CUSTOM_COMMAND_TWO: Parameter(None) -> handler.handle_command_two)
    ];

    let command: json::Value =
        json::json!({ "command": format!("{}+{}", CUSTOM_COMMAND_ONE, CUSTOM_COMMAND_TWO) });

    let response = codec_roundtrip(command, custom_commands).await;
    let expected = json::json!({
        "custom_command_one": [{
            "STATUS": [{
                "Code": 301,
                "Description": "TestMiner v1.0",
                "Msg": "TestMiner custom command 1",
                "STATUS": "S",
                "When": 0
            }],
            "CUSTOM_COMMAND_ONE": [{
                "Attribute": "value",
            }],
            "id": 1
        }],
        "custom_command_two": [{
            "STATUS": [{
                "Code": 45,
                "Description": "TestMiner v1.0",
                "Msg": "Access denied to 'custom_command_two' command",
                "STATUS": "E",
                "When": 0
            }],
            "id": 1
        }],
        "id": 1,
    });

    assert_json_eq(&response, &expected);
}
