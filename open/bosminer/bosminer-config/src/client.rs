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

use crate::error;

use url::Url;

use std::fmt;

use failure::ResultExt;

pub const URL_JAVA_SCRIPT_REGEX: &'static str =
    "^(?:drain|(?:stratum2?\\+tcp)):\\/\\/[\\w\\.-]+:\\d+$";

#[derive(Copy, Clone, Debug)]
pub enum Protocol {
    Drain,
    StratumV1,
    StratumV2,
}

impl Protocol {
    pub const SCHEME_DRAIN: &'static str = "drain";
    pub const SCHEME_STRATUM_V1: &'static str = "stratum+tcp";
    pub const SCHEME_STRATUM_V2: &'static str = "stratum2+tcp";

    pub fn parse(scheme: &str) -> error::Result<Self> {
        Ok(match scheme {
            Self::SCHEME_DRAIN => Self::Drain,
            Self::SCHEME_STRATUM_V1 => Self::StratumV1,
            Self::SCHEME_STRATUM_V2 => Self::StratumV2,
            _ => Err(error::ErrorKind::Client(format!(
                "unknown protocol '{}'",
                scheme
            )))?,
        })
    }

    pub fn scheme(&self) -> &'static str {
        match self {
            Self::Drain => Self::SCHEME_DRAIN,
            Self::StratumV1 => Self::SCHEME_STRATUM_V1,
            Self::StratumV2 => Self::SCHEME_STRATUM_V2,
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Protocol::Drain => write!(f, "Drain"),
            Protocol::StratumV1 => write!(f, "Stratum V1"),
            Protocol::StratumV2 => write!(f, "Stratum V2"),
        }
    }
}

/// Contains basic information about client used for obtaining jobs for solving.
#[derive(Clone, Debug)]
pub struct Descriptor {
    pub protocol: Protocol,
    pub enable: bool,
    pub user: String,
    pub password: Option<String>,
    pub host: String,
    pub port: u16,
}

impl Descriptor {
    pub const USER_INFO_DELIMITER: char = ':';

    pub fn get_url(&self, protocol: bool, port: bool, user: bool) -> String {
        let mut result = if protocol {
            self.protocol.scheme().to_string() + "://"
        } else {
            String::new()
        };
        if user {
            result += format!("{}@", self.user).as_str();
        }
        result += self.host.as_str();
        if port {
            result += format!(":{}", self.port).as_str();
        }

        result
    }

    #[inline]
    pub fn get_full_url(&self) -> String {
        self.get_url(true, true, true)
    }

    /// Create client `Descriptor` from information provided by user.
    pub fn parse(url: &str, user_info: &str) -> error::Result<Self> {
        if user_info.is_empty() {
            Err(error::ErrorKind::Client("empty user info".to_string()))?
        }
        let url = Url::parse(url).context(error::ErrorKind::Client("invalid URL".to_string()))?;

        let protocol = Protocol::parse(url.scheme())?;
        let host = url
            .host()
            .ok_or(error::ErrorKind::Client("missing hostname".to_string()))?
            .to_string();
        let port = url
            .port()
            .ok_or(error::ErrorKind::Client("missing port".to_string()))?;

        // Parse user and password from user info (user[:password])
        let user_info: Vec<_> = user_info.rsplitn(2, Self::USER_INFO_DELIMITER).collect();
        let mut user_info = user_info.iter().rev();

        let user = user_info.next().expect("BUG: missing user").to_string();
        let password = user_info
            .next()
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string());

        Ok(Descriptor {
            protocol,
            enable: true,
            user,
            password,
            host,
            port,
        })
    }
}
