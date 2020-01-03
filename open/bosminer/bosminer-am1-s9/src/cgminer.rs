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

use ii_cgminer_api::command::{FANS, TEMPCTRL, TEMPS};
use ii_cgminer_api::{command, commands, response};

use serde::Serialize;

use std::sync::Arc;

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct TempInfo {
    #[serde(rename = "Board")]
    pub board: f64,
    #[serde(rename = "Chip")]
    pub chip: f64,
}

pub struct Handler;

impl Handler {
    pub fn new() -> Self {
        Self
    }

    async fn handle_temp_ctrl(&self) -> command::Result<response::ext::TempCtrl> {
        Ok(response::ext::TempCtrl {
            mode: "".to_string(),
            target: 0.0,
            hot: 0.0,
            dangerous: 0.0,
        })
    }

    async fn handle_temps(&self) -> command::Result<response::ext::Temps<TempInfo>> {
        Ok(response::ext::Temps {
            list: vec![response::ext::Temp {
                idx: 0,
                id: 0,
                info: TempInfo {
                    board: 0.0,
                    chip: 0.0,
                },
            }],
        })
    }

    async fn handle_fans(&self) -> command::Result<response::ext::Fans> {
        Ok(response::ext::Fans {
            list: vec![response::ext::Fan {
                idx: 0,
                id: 0,
                speed: 0,
                rpm: 0,
            }],
        })
    }
}

pub fn create_custom_commands() -> Option<command::Map> {
    let handler = Arc::new(Handler::new());

    let custom_commands = commands![
        (TEMPCTRL: ParameterLess -> handler.handle_temp_ctrl),
        (TEMPS: ParameterLess -> handler.handle_temps),
        (FANS: ParameterLess -> handler.handle_fans)
    ];

    Some(custom_commands)
}
