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

//! Defines the API command handler (`Handler`)

use super::response;
use super::support::{MultiResponse, Response, ResponseType};

pub use json::Value;
use serde_json as json;

use ii_async_compat::futures::Future;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

/// List of all supported commands.
const POOLS: &str = "pools";
const DEVS: &str = "devs";
const EDEVS: &str = "edevs";
const SUMMARY: &str = "summary";
const VERSION: &str = "version";
const CONFIG: &str = "config";
const DEVDETAILS: &str = "devdetails";
const STATS: &str = "stats";
const ESTATS: &str = "estats";
const CHECK: &str = "check";

pub type Result<T> = std::result::Result<T, response::Error>;

/// A handler to be implemented by the API implementation,
/// takes care of producing a response for each command.
#[async_trait::async_trait]
pub trait Handler: Send + Sync {
    async fn handle_pools(&self) -> Result<response::Pools>;
    async fn handle_devs(&self) -> Result<response::Devs>;
    async fn handle_edevs(&self) -> Result<response::Devs>;
    async fn handle_summary(&self) -> Result<response::Summary>;
    async fn handle_version(&self) -> Result<response::Version>;
    async fn handle_config(&self) -> Result<response::Config>;
    async fn handle_dev_details(&self) -> Result<response::DevDetails>;
    async fn handle_stats(&self) -> Result<response::Stats>;
    async fn handle_estats(&self) -> Result<response::Stats>;
}

/// Holds an incoming API command
pub struct Request {
    value: Value,
}

impl Request {
    pub fn new(value: Value) -> Self {
        Self { value }
    }
}

pub type AsyncHandler = Pin<Box<dyn Future<Output = Result<Response>> + Send + 'static>>;
pub type ParameterLessHandler = Box<dyn Fn() -> AsyncHandler + Send + Sync>;

pub enum HandlerType {
    ParameterLess(ParameterLessHandler),
    // TODO: OneParameter(),
    Check,
}

pub struct Descriptor {
    handler: HandlerType,
}

impl Descriptor {
    pub fn new(_name: &'static str, handler: HandlerType) -> Self {
        Self { handler }
    }
}

macro_rules! command {
    ($name:ident, $handler:expr, $method:ident, ParameterLess) => {{
        let handler = $handler.clone();
        let f: ParameterLessHandler = Box::new(move || {
            let handler = handler.clone();
            Box::pin(async move { handler.$method().await.map(|response| response.into()) })
        });
        HandlerType::ParameterLess(f)
    }};
}

macro_rules! commands {
    () => (
        HashMap::new()
    );
    ($(($name:ident, $handler:expr, $method:ident, $type:ident)),+) => {
        {
            let mut map = HashMap::new();
            $(
                let command = command!($name, $handler, $method, $type);
                map.insert($name, Descriptor::new($name, command));
            )*
            map
        }
    }
}

pub struct Receiver {
    commands: HashMap<&'static str, Descriptor>,
}

impl Receiver {
    pub fn new<T>(handler: T) -> Self
    where
        T: Handler + 'static,
    {
        let handler = Arc::new(handler);

        // add generic commands
        let mut commands = commands![
            (POOLS, handler, handle_pools, ParameterLess),
            (DEVS, handler, handle_devs, ParameterLess),
            (EDEVS, handler, handle_edevs, ParameterLess),
            (SUMMARY, handler, handle_summary, ParameterLess),
            (VERSION, handler, handle_version, ParameterLess),
            (CONFIG, handler, handle_config, ParameterLess),
            (DEVDETAILS, handler, handle_dev_details, ParameterLess),
            (STATS, handler, handle_stats, ParameterLess),
            (ESTATS, handler, handle_estats, ParameterLess)
        ];

        // add special built-in commands
        commands.insert(CHECK, Descriptor::new(CHECK, HandlerType::Check));

        Self { commands }
    }

    fn handle_check(&self, parameter: Option<&Value>) -> Result<response::Check> {
        let command =
            parameter.ok_or_else(|| response::Error::new(response::StatusCode::MissingCheckCmd))?;
        let result = match command {
            Value::String(command) => self.commands.get(command.as_str()).into(),
            _ => response::Bool::N,
        };

        Ok(response::Check {
            exists: result,
            access: result,
        })
    }

    pub async fn handle_single(&self, command: &str, parameter: Option<&Value>) -> Response {
        let response = match self.commands.get(command) {
            Some(descriptor) => match &descriptor.handler {
                HandlerType::ParameterLess(handle) => handle().await,
                HandlerType::Check => self.handle_check(parameter).map(|response| response.into()),
            },
            None => Err(response::Error::new(response::StatusCode::InvalidCommand)),
        };

        response.unwrap_or_else(|error| error.into())
    }

    pub async fn handle(&self, command_request: Request) -> ResponseType {
        let command = match command_request.value.get("command").and_then(Value::as_str) {
            None => {
                return ResponseType::Single(
                    response::Error::new(response::StatusCode::MissingCommand).into(),
                )
            }
            Some(value) => value,
        };
        let parameter = command_request.value.get("parameter");

        if !command.contains('+') {
            ResponseType::Single(self.handle_single(command, parameter).await)
        } else {
            let mut responses = MultiResponse::new();
            for cmd in command.split('+') {
                // TODO: check for param which prohibited when multi-response is used
                let response = self.handle_single(cmd, parameter).await;
                let response =
                    json::to_value(&response).expect("BUG: cannot serialize response to JSON");
                responses.add_response(cmd, response);
            }
            ResponseType::Multi(responses)
        }
    }
}
