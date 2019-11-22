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
use crate::hub;
use crate::node;
use crate::work;

use futures::lock::{Mutex, MutexGuard};
use ii_async_compat::futures;

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
#[derive(Debug)]
pub struct Descriptor {
    pub url: String,
    pub user: String,
    pub protocol: Protocol,
    pub socket_addr: SocketAddr,
}

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

pub struct Handle {
    pub node: Arc<dyn node::Client>,
    #[allow(dead_code)]
    engine_sender: Arc<work::EngineSender>,
}

impl Handle {
    pub fn new<T>(client: T, engine_sender: Arc<work::EngineSender>) -> Self
    where
        T: node::Client + 'static,
    {
        Self {
            node: Arc::new(client),
            engine_sender,
        }
    }
}

impl PartialEq for Handle {
    fn eq(&self, other: &Handle) -> bool {
        Arc::ptr_eq(&self.node, &other.node)
    }
}

/// Keeps track of all active clients
pub struct Registry {
    list: Mutex<Vec<Arc<Handle>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            list: Mutex::new(vec![]),
        }
    }

    pub async fn register_client(&self, client: Handle) {
        let container = &mut *self.list.lock().await;
        assert!(
            container
                .iter()
                .find(|old| old.as_ref() == &client)
                .is_none(),
            "BUG: client already present in the registry"
        );
        container.push(Arc::new(client));
    }

    pub async fn lock_clients<'a>(&'a self) -> MutexGuard<'a, Vec<Arc<Handle>>> {
        self.list.lock().await
    }
}

/// Register client that implements a protocol set in `descriptor`
pub async fn register(core: &Arc<hub::Core>, descriptor: Descriptor) -> Arc<dyn node::Client> {
    // NOTE: the match statement needs to be updated in case of multiple protocol support
    core.add_client(|job_solver| match descriptor.protocol {
        Protocol::StratumV2 => stratum_v2::StratumClient::new(descriptor, job_solver),
    })
    .await
}
