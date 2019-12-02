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

#[tokio::test]
async fn test_api_basic() {
    let resp = codec_roundtrip("{\"command\":\"version\"}\n").await;

    assert_eq!(
        resp,
        json::json!({
            "STATUS": [{
                "STATUS": "S",
                "When": 0,
                "Code": 0,
                "Msg": "CGMiner versions",
                "Description": ""
            }],
            "VERSION": [{
                "API": "3.7",
                "CGMiner": "bOSminer_am1-s9-20190605-0_0de55997"
            }],
            "id": 1
        })
    );
}

#[tokio::test]
async fn test_api_multiple() {
    let resp = codec_roundtrip("{\"command\":\"version+config\"}\n").await;

    assert_eq!(
        resp,
        json::json!({
            "config": {
                "STATUS": [{
                    "Code": 0,
                    "Description": "",
                    "Msg": "CGMiner config",
                    "STATUS": "S",
                    "When": 0
                }],
                "VERSION": [{
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
                    "Code": 0,
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
        })
    );
}
