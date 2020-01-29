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

pub const SCHEME_STRATUM_V2: &str = "stratum2+tcp";

#[derive(Copy, Clone, Debug)]
pub enum Protocol {
    StratumV2,
}

impl Protocol {
    pub fn parse(scheme: &str) -> error::Result<Self> {
        if scheme == SCHEME_STRATUM_V2 {
            Ok(Self::StratumV2)
        } else {
            Err(error::ErrorKind::Client("unknown protocol".to_string()))?
        }
    }

    pub fn scheme(&self) -> &'static str {
        match self {
            Self::StratumV2 => SCHEME_STRATUM_V2,
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Protocol::StratumV2 => write!(f, "Stratum V2"),
        }
    }
}

/// Contains basic information about client used for obtaining jobs for solving.
#[derive(Clone, Debug)]
pub struct Descriptor {
    protocol: Protocol,
    user: String,
    host: String,
    port: u16,
}

impl Descriptor {
    #[inline]
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    #[inline]
    pub fn host(&self) -> String {
        self.host.clone()
    }

    #[inline]
    pub fn user(&self) -> String {
        self.user.clone()
    }

    #[inline]
    pub fn port(&self) -> u16 {
        self.port
    }

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
    pub fn parse(url: &str, user: &str) -> error::Result<Self> {
        if user.is_empty() {
            Err(error::ErrorKind::Client("empty user".to_string()))?
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

        Ok(Descriptor {
            protocol,
            user: user.to_string(),
            host,
            port,
        })
    }
}
