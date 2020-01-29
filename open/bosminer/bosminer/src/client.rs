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

mod scheduler;

// Sub-modules with client implementation
pub mod stratum_v2;

use crate::hub;
use crate::node;
use crate::work;

// Scheduler re-exports
pub use scheduler::JobExecutor;

use bosminer_config::client::{Descriptor, Protocol};

use futures::channel::mpsc;
use ii_async_compat::futures;

use std::slice;
use std::sync::Arc;

#[derive(Debug)]
pub struct Handle {
    pub node: Arc<dyn node::Client>,
    pub engine_sender: Arc<work::EngineSender>,
    pub solution_sender: mpsc::UnboundedSender<work::Solution>,
}

impl Handle {
    pub fn new<T>(
        client: T,
        engine_sender: Arc<work::EngineSender>,
        solution_sender: mpsc::UnboundedSender<work::Solution>,
    ) -> Self
    where
        T: node::Client + 'static,
    {
        Self {
            node: Arc::new(client),
            engine_sender,
            solution_sender,
        }
    }

    /// Tests if solution should be delivered to this client
    /// NOTE: This comparison uses trait method `node::Info::get_unique_ptr` to unify dynamic
    /// objects to point to the same pointer otherwise direct comparison of self with other is never
    /// satisfied even if the dynamic objects are same.
    pub fn matching_solution(&self, solution: &work::Solution) -> bool {
        Arc::ptr_eq(
            &self.node.clone().get_unique_ptr(),
            &solution.origin().get_unique_ptr(),
        )
    }
}

impl PartialEq for Handle {
    fn eq(&self, other: &Handle) -> bool {
        Arc::ptr_eq(
            &self.node.clone().get_unique_ptr(),
            &other.node.clone().get_unique_ptr(),
        )
    }
}

/// Keeps track of all active clients
pub struct Registry {
    list: Vec<scheduler::ClientHandle>,
}

impl Registry {
    pub fn new() -> Self {
        Self { list: vec![] }
    }

    pub fn count(&self) -> usize {
        self.list.len()
    }

    fn iter(&self) -> slice::Iter<scheduler::ClientHandle> {
        self.list.iter()
    }

    fn iter_mut(&mut self) -> slice::IterMut<scheduler::ClientHandle> {
        self.list.iter_mut()
    }

    pub fn get_clients(&self) -> Vec<Arc<dyn node::Client>> {
        self.iter()
            .map(|client| client.handle.node.clone())
            .collect()
    }

    fn register_client(&mut self, client: scheduler::ClientHandle) -> &scheduler::ClientHandle {
        assert!(
            self.list.iter().find(|old| *old == &client).is_none(),
            "BUG: client already present in the registry"
        );
        self.list.push(client);
        self.list.last().expect("BUG: client list is empty")
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
