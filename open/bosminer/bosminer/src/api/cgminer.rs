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

//! This module implements CGMiner compatible API server to control bOSminer and to extract
//! statistics from it.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::SystemTime;

use ii_async_compat::{futures, tokio, tokio_util, bytes};

use bytes::BytesMut;
use futures::{SinkExt, StreamExt};
use tokio::time::delay_for;
use tokio_util::codec::{Decoder, Encoder, LinesCodec, LinesCodecError};

pub use json::Value;
use serde::{Serialize, Serializer};
use serde_json as json;

struct Timestamp(AtomicBool);

impl Timestamp {
    const fn new() -> Self {
        Self(AtomicBool::new(true))
    }

    fn enable(&self, enable: bool) {
        self.0.store(enable, Ordering::Relaxed);
    }

    fn get(&self) -> u32 {
        if self.0.load(Ordering::Relaxed) {
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|duration| duration.as_secs() as u32)
                .unwrap_or(0)
        } else {
            0
        }
    }
}

static TIMESTAMP: Timestamp = Timestamp::new();

// === Response data structures

#[derive(Serialize, Eq, PartialEq, Clone, Debug)]
pub enum Status {
    W,
    I,
    S,
    E,
}

#[derive(Serialize, PartialEq, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
struct StatusInfo {
    #[serde(rename = "STATUS")]
    pub status: Status,
    pub when: u32,
    pub code: u32,
    pub msg: String,
    pub description: String,
}

impl StatusInfo {
    fn new(succes: bool, code: u32, msg: String) -> Self {
        Self {
            status: if succes { Status::S } else { Status::E },
            when: TIMESTAMP.get(),
            code,
            msg,
            description: String::new(), // FIXME: Miner ID (?)
        }
    }
}

#[derive(Debug)]
pub struct Response {
    status: StatusInfo,
    response: Value,
    name: &'static str,
    id: usize,
}

impl Response {
    pub fn new<S: Serialize>(
        response: S,
        name: &'static str,
        success: bool,
        code: u32,
        msg: String,
    ) -> Self {
        let status = StatusInfo::new(success, code, msg);
        let response = json::to_value(response).expect("Response serialization failed");

        Self {
            status,
            response,
            name,
            id: 1,
        }
    }
}

impl Serialize for Response {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("STATUS", &[&self.status])?;
        map.serialize_entry("id", &self.id)?;
        map.serialize_entry(&self.name, &[&self.response])?;
        map.end()
    }
}

#[derive(Serialize, Debug)]
struct MultiResponse {
    id: usize,

    #[serde(flatten)]
    responses: HashMap<String, Value>,
}

impl MultiResponse {
    fn new() -> Self {
        Self {
            id: 0,
            responses: HashMap::new(),
        }
    }

    fn add_response(&mut self, name: &str, resp: Value) {
        self.responses.insert(name.to_string(), resp);
    }
}

#[derive(Serialize, Debug)]
#[serde(untagged)]
enum ResponseSet {
    Single(Response),
    Multi(MultiResponse),
}

// === Responses

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

// === Command handler

#[async_trait::async_trait]
pub trait Handler: Sync + Send {
    async fn handle_version(&self) -> Option<Response>;
    async fn handle_config(&self) -> Option<Response>;
}

// === Command

pub struct Command(Value);

impl Command {
    async fn handle_single(
        &self,
        cmd: &str,
        param: Option<&Value>,
        handler: &dyn Handler,
    ) -> Option<Response> {
        match cmd {
            "version" => handler.handle_version().await,
            "config" => handler.handle_config().await,
            _ => None,
        }
    }

    async fn handle(&self, handler: &dyn Handler) -> Option<ResponseSet> {
        let cmd = self.0.get("command").and_then(Value::as_str)?;
        let param = self.0.get("parameter");

        if !cmd.contains('+') {
            self.handle_single(cmd, param, handler)
                .await
                .map(ResponseSet::Single)
        } else {
            let mut responses = MultiResponse::new();

            for cmd in cmd.split('+') {
                let resp = self.handle_single(cmd, param, handler).await?;
                let resp = json::to_value(&resp).ok()?;
                responses.add_response(cmd, resp);
            }

            Some(ResponseSet::Multi(responses))
        }
    }
}

// === Codec

#[derive(Default, Debug)]
struct Codec(LinesCodec);

fn no_max_line_lengh(err: LinesCodecError) -> io::Error {
    match err {
        LinesCodecError::Io(io) => io,
        LinesCodecError::MaxLineLengthExceeded => unreachable!(),
    }
}

impl Decoder for Codec {
    type Item = Command;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let line = self.0.decode(src).map_err(no_max_line_lengh)?;

        if let Some(line) = line {
            json::from_str(line.as_str())
                .map(Command)
                .map(Option::Some)
                .map_err(Into::into)
        } else {
            Ok(None)
        }
    }
}

impl Encoder for Codec {
    type Item = ResponseSet;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let line = json::to_string(&item)?;
        self.0.encode(line, dst).map_err(no_max_line_lengh)
    }
}

// === Framing

#[derive(Debug)]
struct Framing;

impl ii_wire::Framing for Framing {
    type Tx = ResponseSet;
    type Rx = Command;
    type Error = io::Error;
    type Codec = Codec;
}

// === Server

type Server = ii_wire::Server<Framing>;
type Connection = ii_wire::Connection<Framing>;

async fn handle_connection(mut conn: Connection, handler: Arc<dyn Handler>) {
    while let Some(Ok(command)) = conn.next().await {
        if let Some(resp) = command.handle(&*handler).await {
            match conn.tx.send(resp).await {
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
}

pub async fn run(handler: Arc<dyn Handler>, listen_addr: SocketAddr) -> io::Result<()> {
    let mut server = Server::bind(&listen_addr)?;

    while let Some(conn) = server.next().await {
        if let Ok(conn) = conn {
            tokio::spawn(handle_connection(conn, handler.clone()));
        }
    }

    Ok(())
}

#[cfg(test)]
pub mod test {
    use bytes::BytesMut;

    use super::*;

    struct TestHandler;

    #[async_trait::async_trait]
    impl Handler for TestHandler {
        async fn handle_version(&self) -> Option<Response> {
            Some(
                Version {
                    cgminer: "bOSminer_am1-s9-20190605-0_0de55997".to_string(),
                    api: "3.7".to_string(),
                }
                .into(),
            )
        }

        async fn handle_config(&self) -> Option<Response> {
            let config = Config {
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
}
