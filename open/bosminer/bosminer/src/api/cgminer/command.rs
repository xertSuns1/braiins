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

pub use json::Value;
use serde_json as json;

use super::response;
use super::{MultiResponse, Response, ResponseType};

pub type Result<T> = std::result::Result<T, response::Error>;

/// A handler to be implemented by the API implementation,
/// takes care of producing a response for each command.
#[async_trait::async_trait]
pub trait Handler: Sync + Send {
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

/// Holds an incomming API command
pub struct Command(Value);

impl Command {
    pub fn new(json: Value) -> Self {
        Self(json)
    }

    fn handle_check(&self, _parameter: Option<&Value>) -> Result<response::Check> {
        Err(response::Error::new(response::StatusCode::MissingCheckCmd))
    }

    pub async fn handle_single(
        &self,
        command: &str,
        parameter: Option<&Value>,
        handler: &dyn Handler,
    ) -> Response {
        let response = match command {
            "pools" => handler.handle_pools().await.map(|response| response.into()),
            "devs" => handler.handle_devs().await.map(|response| response.into()),
            "edevs" => handler.handle_edevs().await.map(|response| response.into()),
            "summary" => handler
                .handle_summary()
                .await
                .map(|response| response.into()),
            "version" => handler
                .handle_version()
                .await
                .map(|response| response.into()),
            "config" => handler
                .handle_config()
                .await
                .map(|response| response.into()),
            "devdetails" => handler
                .handle_dev_details()
                .await
                .map(|response| response.into()),
            "stats" => handler.handle_stats().await.map(|response| response.into()),
            "estats" => handler
                .handle_estats()
                .await
                .map(|response| response.into()),
            "check" => self.handle_check(parameter).map(|response| response.into()),
            _ => Err(response::Error::new(response::StatusCode::InvalidCommand)),
        };
        response.unwrap_or_else(|error| error.into())
    }

    pub async fn handle(&self, handler: &dyn Handler) -> ResponseType {
        let command = match self.0.get("command").and_then(Value::as_str) {
            None => {
                return ResponseType::Single(
                    response::Error::new(response::StatusCode::MissingCommand).into(),
                )
            }
            Some(value) => value,
        };
        let parameter = self.0.get("parameter");

        if !command.contains('+') {
            ResponseType::Single(self.handle_single(command, parameter, handler).await)
        } else {
            let mut responses = MultiResponse::new();
            for cmd in command.split('+') {
                // TODO: check for param which prohibited when multi-response is used
                let response = self.handle_single(cmd, parameter, handler).await;
                let response =
                    json::to_value(&response).expect("BUG: cannot serialize response to JSON");
                responses.add_response(cmd, response);
            }
            ResponseType::Multi(responses)
        }
    }
}
