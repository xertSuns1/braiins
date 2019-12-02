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

use ii_async_compat::{bytes, tokio, tokio_util};
use tokio_util::codec::Decoder;

use bytes::BytesMut;

use json::Value;
use serde_json as json;

use super::server::Codec;
use super::*;

struct TestHandler;

#[async_trait::async_trait]
impl Handler for TestHandler {
    async fn handle_version(&self) -> Option<Response> {
        Some(
            response::Version {
                cgminer: "bOSminer_am1-s9-20190605-0_0de55997".to_string(),
                api: "3.7".to_string(),
            }
            .into(),
        )
    }

    async fn handle_config(&self) -> Option<Response> {
        let config = response::Config {
            asc_count: 0,
            pga_count: 0,
            pool_count: 0,
            strategy: "Failover".to_string(),
            log_interval: 0,
            device_code: String::new(),
            os: "Braiins OS".to_string(),
            hotplug: "None".to_string(),
        };

        Some(config.into())
    }
}

async fn codec_roundtrip(command: &str) -> Value {
    TIMESTAMP.enable(false);

    let handler = TestHandler;
    let mut codec = Codec::default();

    let mut command_buf = BytesMut::with_capacity(256);
    command_buf.extend_from_slice(command.as_bytes());

    let command = codec.decode(&mut command_buf).unwrap().unwrap();
    let resp = command.handle(&handler).await.expect("Handler failed");
    json::to_value(&resp).unwrap()
}

type JsonMap = json::Map<String, Value>;

fn json_map_diff(a: &JsonMap, b: &JsonMap) -> JsonMap {
    let mut res = json::Map::new();

    // Check `a` keys
    for (ak, av) in a.iter() {
        if let Some(bv) = b.get(ak) {
            let diff = json_diff(av, bv);
            if !diff.is_null() {
                res.insert(ak.clone(), diff);
            }
        } else {
            res.insert(ak.clone(), Value::Bool(true));
        }
    }

    // Check `b` keys not present in `a`
    for bk in b.keys() {
        if let None = a.get(bk) {
            res.insert(bk.clone(), Value::Bool(true));
        }
    }

    res
}

/// Computes a difference of two json `Value`s.
/// Returns `Value::Null` if the two are equal,
/// otherwise return `true` or an object in which each non-equal subvalue
/// is marked `true`.
fn json_diff(a: &Value, b: &Value) -> Value {
    match a {
        Value::Object(a) => match b {
            Value::Object(b) => {
                let map_diff = json_map_diff(a, b);

                if map_diff.is_empty() {
                    Value::Null
                } else {
                    Value::Object(map_diff)
                }
            }
            _ => Value::Bool(true),
        },
        _ => {
            if a == b {
                Value::Null
            } else {
                Value::Bool(true)
            }
        }
    }
}

fn assert_json_eq(a: &Value, b: &Value) {
    let diff = json_diff(a, b);
    if !diff.is_null() {
        panic!(
            "Assertion failed: JSON valued not equal:\na: {}\nb: {}\ndifference: {}",
            a, b, diff
        );
    }
}

#[tokio::test]
async fn test_api_basic() {
    let resp = codec_roundtrip("{\"command\":\"version\"}\n").await;

    let expected = json::json!({
        "STATUS": [{
            "STATUS": "S",
            "When": 0,
            "Code": 22,
            "Msg": "CGMiner versions",
            "Description": ""
        }],
        "VERSION": [{
            "API": "3.7",
            "CGMiner": "bOSminer_am1-s9-20190605-0_0de55997"
        }],
        "id": 1
    });

    assert_json_eq(&resp, &expected);
}

#[tokio::test]
async fn test_api_multiple() {
    let resp = codec_roundtrip("{\"command\":\"version+config\"}\n").await;

    let expected = json::json!({
        "config": {
            "STATUS": [{
                "Code": 33,
                "Description": "",
                "Msg": "CGMiner config",
                "STATUS": "S",
                "When": 0
            }],
            "CONFIG": [{
                "ASC Count": 0,
                "Device Code:": "",
                "Hotplug": "None",
                "Log Interval": 0,
                "OS": "Braiins OS",
                "PGA Count": 0,
                "Pool Count": 0,
                "Strategy": "Failover"
            }],
            "id": 1
        },
        "id": 0,
        "version": {
            "STATUS": [{
                "Code": 22,
                "Description": "",
                "Msg": "CGMiner versions",
                "STATUS": "S",
                "When": 0
            }],
            "VERSION": [{
                "API": "3.7",
                "CGMiner": "bOSminer_am1-s9-20190605-0_0de55997"
            }],
            "id": 1
        }
    });

    assert_json_eq(&resp, &expected);
}
