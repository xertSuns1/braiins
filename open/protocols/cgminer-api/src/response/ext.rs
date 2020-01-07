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

//! Defines all extended bOSminer API responses

use super::*;

/// Basic temperature control settings
#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct TempCtrl {
    #[serde(rename = "Mode")]
    pub mode: String, // TODO: use enum
    /// Temperature setpoint
    #[serde(rename = "Target")]
    pub target: f64,
    /// Hot temperature threshold is typically intended to warn the user
    #[serde(rename = "Hot")]
    pub hot: f64,
    /// Dangerous temperature is recommended to result in shutdown to prevent hardware damage
    /// from overheating
    #[serde(rename = "Dangerous")]
    pub dangerous: f64,
}

impl From<TempCtrl> for Dispatch {
    fn from(temp_ctrl: TempCtrl) -> Self {
        Dispatch::from_success(
            vec![temp_ctrl],
            "TEMPCTRL",
            StatusCode::TempCtrl.into(),
            "Temperature control".to_string(),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Temp<T> {
    #[serde(rename = "TEMP")]
    pub idx: i32,
    #[serde(rename = "ID")]
    pub id: i32,
    #[serde(flatten)]
    pub info: T,
}

pub struct Temps<T> {
    pub list: Vec<Temp<T>>,
}

impl<T> From<Temps<T>> for Dispatch
where
    T: serde::Serialize,
{
    fn from(temps: Temps<T>) -> Self {
        let temp_count = temps.list.len();
        Dispatch::from_success(
            temps.list,
            "TEMPS",
            StatusCode::Temps.into(),
            format!("{} Temp(s)", temp_count),
        )
    }
}

#[derive(Serialize, PartialEq, Clone, Debug)]
pub struct Fan {
    #[serde(rename = "FAN")]
    pub idx: i32,
    #[serde(rename = "ID")]
    pub id: i32,
    #[serde(rename = "Speed")]
    pub speed: u32,
    #[serde(rename = "RPM")]
    pub rpm: u32,
}

pub struct Fans {
    pub list: Vec<Fan>,
}

impl From<Fans> for Dispatch {
    fn from(fans: Fans) -> Self {
        let fan_count = fans.list.len();
        Dispatch::from_success(
            fans.list,
            "FANS",
            StatusCode::Fans.into(),
            format!("{} Fan(s)", fan_count),
        )
    }
}
