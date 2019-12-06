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

//! Defines support structures for API responses serialization

use super::response;
use super::TIMESTAMP;

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use serde::{Serialize, Serializer};
use serde_json as json;
use serde_json::Value;

/// Flag whether a real timestamp should be used when serializing.
/// When turned off, a timestamp of 0 is used instad, this is useful for tests.
pub struct Timestamp(AtomicBool);

impl Timestamp {
    pub const fn new() -> Self {
        Self(AtomicBool::new(true))
    }

    #[allow(dead_code)]
    pub fn enable(&self, enable: bool) {
        self.0.store(enable, Ordering::Relaxed);
    }

    pub fn get(&self) -> u32 {
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

/// Generic container for any response, ensures conforming serialization
#[derive(Debug)]
pub struct Response {
    status: response::StatusInfo,
    responses: Option<(&'static str, Value)>,
    id: usize,
}

impl Response {
    pub fn from_success<S: Serialize>(
        responses: Vec<S>,
        name: &'static str,
        code: response::StatusCode,
        msg: String,
    ) -> Self {
        let responses = json::to_value(responses).expect("Response serialization failed");

        Self {
            status: Self::create_status_info(response::Status::S, code, msg),
            responses: Some((name, responses)),
            id: 1,
        }
    }

    fn create_status_info(
        status: response::Status,
        code: response::StatusCode,
        msg: String,
    ) -> response::StatusInfo {
        response::StatusInfo {
            status,
            when: TIMESTAMP.get(),
            code,
            msg,
            description: String::new(), // FIXME: Miner ID (?)
        }
    }
}

impl From<response::Error> for Response {
    fn from(error: response::Error) -> Response {
        Self {
            status: Self::create_status_info(response::Status::E, error.code, error.msg()),
            responses: None,
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
        if let Some((name, responses)) = &self.responses {
            map.serialize_entry(name, responses)?;
        }
        map.serialize_entry("id", &self.id)?;
        map.end()
    }
}

/// Container for a multi-reponse
#[derive(Serialize, Debug)]
pub struct MultiResponse {
    #[serde(flatten)]
    responses: HashMap<String, Value>,
    id: usize,
}

impl MultiResponse {
    pub fn new() -> Self {
        Self {
            responses: HashMap::new(),
            id: 1,
        }
    }

    pub fn add_response(&mut self, name: &str, response: Value) {
        self.responses
            .insert(name.to_string(), Value::Array(vec![response]));
    }
}

/// Wrapper that discriminates either a single response or a collection
/// of multiple responses, ensuring conforming serialization
#[derive(Serialize, Debug)]
#[serde(untagged)]
pub enum ResponseType {
    Single(Response),
    Multi(MultiResponse),
}
