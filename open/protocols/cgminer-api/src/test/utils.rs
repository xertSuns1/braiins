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

use crate::command;
use crate::response;
use crate::support;
use crate::Codec;

use ii_async_compat::{bytes, tokio_util};
use tokio_util::codec::Decoder;

use bytes::BytesMut;

use json::Value;
use serde_json as json;

struct ZeroTime;

impl support::When for ZeroTime {
    fn when() -> response::Time {
        0
    }
}

pub async fn codec_roundtrip<T>(command: json::Value, custom_commands: T) -> Value
where
    T: Into<Option<command::Map>>,
{
    let command_receiver = command::Receiver::<ZeroTime>::new(
        super::handler::BasicTest,
        "TestMiner".to_string(),
        "v1.0".to_string(),
        custom_commands,
    );
    let mut codec = Codec::default();

    let mut command_buf = BytesMut::with_capacity(256);
    command_buf.extend_from_slice(command.to_string().as_bytes());

    let command = codec.decode(&mut command_buf).unwrap().unwrap();
    let response = command_receiver.handle(command).await;
    json::to_value(&response).unwrap()
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

pub fn assert_json_eq(a: &Value, b: &Value) {
    let diff = json_diff(a, b);
    if !diff.is_null() {
        panic!(
            "Assertion failed: JSON valued not equal:\na: {}\nb: {}\ndifference: {}",
            a, b, diff
        );
    }
}
