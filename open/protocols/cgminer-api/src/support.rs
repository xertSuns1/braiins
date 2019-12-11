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

use crate::response;

use serde::{Serialize, Serializer};
use serde_json as json;

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

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

pub trait ValueExt {
    fn to_i32(&self) -> Option<i32>;

    fn is_i32(&self) -> bool {
        self.to_i32().is_some()
    }
}

/// Support CGMiner specific type conversions
impl ValueExt for json::Value {
    fn to_i32(&self) -> Option<i32> {
        match self {
            json::Value::Number(value) if value.is_u64() => {
                let number = value.as_u64().expect("BUG: cannot convert json number");
                Some(number as i32)
            }
            json::Value::Number(value) if value.is_i64() => {
                let number = value.as_i64().expect("BUG: cannot convert json number");
                Some(number as i32)
            }
            json::Value::Number(value) if value.is_f64() => {
                let number = value.as_f64().expect("BUG: cannot convert json number");
                Some(number as i32)
            }
            // TODO: cgminer tries to parse all possible types if they contains some number
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct SingleResponse {
    pub status_info: response::StatusInfo,
    pub body: Option<(&'static str, json::Value)>,
}

impl Serialize for SingleResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("STATUS", &[&self.status_info])?;
        if let Some((name, responses)) = &self.body {
            map.serialize_entry(name, responses)?;
        }
        map.serialize_entry("id", &1)?;
        map.end()
    }
}

/// Container for a multi-response
#[derive(Serialize, Debug)]
pub struct MultiResponse {
    #[serde(flatten)]
    responses: HashMap<String, Vec<SingleResponse>>,
    id: usize,
}

impl MultiResponse {
    pub fn new() -> Self {
        Self {
            responses: HashMap::new(),
            id: 1,
        }
    }

    pub fn add_response(&mut self, name: &str, response: SingleResponse) {
        self.responses.insert(name.to_string(), vec![response]);
    }
}

/// Wrapper that discriminates either a single response or a collection
/// of multiple responses, ensuring conforming serialization
#[derive(Serialize, Debug)]
#[serde(untagged)]
pub enum ResponseType {
    Single(SingleResponse),
    Multi(MultiResponse),
}
