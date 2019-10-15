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

//! This module contains common functionality related to mining protocol client and allows
//! executing a specific type of mining protocol client instance.

pub mod stratum_v2;

use crate::error;
use crate::job;
use crate::node;

use std::fmt;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;

use failure::ResultExt;

#[derive(Copy, Clone, Debug)]
pub enum Protocol {
    StratumV2,
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Protocol::StratumV2 => write!(f, "Stratum V2"),
        }
    }
}

/// Contains basic information about client used for obtaining jobs for solving.
/// It is also used for statistics measurement.
#[derive(Debug)]
pub struct Descriptor {
    pub url: String,
    pub user: String,
    pub protocol: Protocol,
    pub socket_addr: SocketAddr,
}

impl fmt::Display for Descriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{} ({})", self.url, self.user, self.protocol)
    }
}

/// Implement node info for `Descriptor` to use it for identification of unique job origin and
/// statistics measurement.
impl node::Info for Descriptor {}

/// Create client `Descriptor` from information provided by user.
pub fn parse(url: String, user: String) -> error::Result<Descriptor> {
    let socket_addr = url
        .to_socket_addrs()
        .context("Invalid server address")?
        .next()
        .ok_or("Cannot resolve any IP address")?;

    Ok(Descriptor {
        url,
        user,
        protocol: Protocol::StratumV2,
        socket_addr,
    })
}

/// Run relevant client implementing a protocol set in `Descriptor`
pub async fn run(job_solver: job::Solver, descriptor: Descriptor) {
    match descriptor.protocol {
        Protocol::StratumV2 => stratum_v2::run(job_solver, Arc::new(descriptor)).await,
    };
}
