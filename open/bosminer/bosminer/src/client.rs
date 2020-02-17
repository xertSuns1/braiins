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
pub mod drain;
pub mod stratum_v2;
pub mod stratum_v2_channels;

use crate::error;
use crate::hal;
use crate::job;
use crate::node;
use crate::stats;
use crate::work;

// Scheduler re-exports
pub use scheduler::JobExecutor;

use bosminer_config::{ClientDescriptor, ClientProtocol, GroupDescriptor};

use futures::channel::mpsc;
use futures::lock::Mutex;
use ii_async_compat::futures;

use std::slice;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub struct Handle {
    // Basic information about client used for connection to remote server
    pub descriptor: ClientDescriptor,
    node: Arc<dyn node::Client>,
    enabled: AtomicBool,
    engine_sender: Arc<work::EngineSender>,
    solution_sender: mpsc::UnboundedSender<work::Solution>,
}

impl Handle {
    /// `channel` - endpoints for 2 channels so that stratum V2 client can communicate with an
    /// external client that implements some protocol extension
    fn new(
        descriptor: ClientDescriptor,
        backend_info: Option<hal::BackendInfo>,
        channel: Option<(
            stratum_v2::ExtensionChannelToStratumReceiver,
            stratum_v2::ExtensionChannelFromStratumSender,
        )>,
    ) -> Self {
        let (solution_sender, solution_receiver) = mpsc::unbounded();
        // Initially register new client without ability to send work
        let engine_sender = Arc::new(work::EngineSender::new(None));

        let job_solver = job::Solver::new(engine_sender.clone(), solution_receiver);
        let node: Arc<dyn node::Client> = match &descriptor.protocol {
            ClientProtocol::Drain => {
                assert!(
                    channel.is_none(),
                    "BUG: protocol 'Drain' does not support channel"
                );
                Arc::new(drain::Client::new(descriptor.get_full_url(), job_solver))
            }
            ClientProtocol::StratumV1 => {
                assert!(
                    channel.is_none(),
                    "BUG: protocol 'Stratum V1' does not support channel"
                );
                Arc::new(stratum_v2_channels::StratumClient::new(
                    stratum_v2_channels::ConnectionDetails::from_descriptor(&descriptor),
                    job_solver,
                ))
            }
            ClientProtocol::StratumV2 => Arc::new(stratum_v2::StratumClient::new(
                stratum_v2::ConnectionDetails::from_descriptor(&descriptor),
                backend_info,
                job_solver,
                channel,
            )),
        };

        Self {
            descriptor,
            node,
            enabled: AtomicBool::new(false),
            engine_sender,
            solution_sender,
        }
    }

    pub fn from_descriptor(
        descriptor: ClientDescriptor,
        backend_info: Option<hal::BackendInfo>,
    ) -> Self {
        Handle::new(descriptor, backend_info, None)
    }

    pub fn from_config(
        client_config: hal::ClientConfig,
        backend_info: Option<hal::BackendInfo>,
    ) -> Self {
        Handle::new(
            client_config.descriptor,
            backend_info,
            client_config.channel,
        )
    }

    pub fn replace_engine_generator(
        &self,
        engine_generator: work::EngineGenerator,
    ) -> work::EngineGenerator {
        self.engine_sender
            .replace_engine_generator(engine_generator)
    }

    /// Tests if solution should be delivered to this client
    /// NOTE: This comparison uses trait method `node::Info::get_unique_ptr` to unify dynamic
    /// objects to point to the same pointer otherwise direct comparison of self with other is never
    /// satisfied even if the dynamic objects are same.
    pub fn matching_solution(&self, solution: &work::Solution) -> bool {
        solution
            .origin()
            .upgrade()
            .map(|origin| {
                Arc::ptr_eq(
                    &self.node.clone().get_unique_ptr(),
                    &origin.get_unique_ptr(),
                )
            })
            .unwrap_or(false)
    }

    #[inline]
    pub fn is_running(&self) -> bool {
        self.is_enabled() && self.status() == crate::sync::Status::Running
    }

    #[inline]
    pub fn status(&self) -> crate::sync::Status {
        self.node.status().status()
    }

    #[inline]
    fn start(&self) {
        if self.node.status().initiate_starting() {
            // The client can be started safely
            self.node.clone().start();
        }
    }

    #[inline]
    fn stop(&self) {
        if self.node.status().initiate_stopping() {
            // The client can be stopped safely
            self.node.clone().stop();
        }
    }

    /// Check if current state of the client is enabled
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Try to enable the client. Default client state should be disabled.
    pub(crate) fn try_enable(&self) -> Result<(), ()> {
        let was_enabled = self.enabled.swap(true, Ordering::Relaxed);
        if !was_enabled {
            // Immediately start the client when it was disabled
            // TODO: force the scheduler
            self.start();
            Ok(())
        } else {
            Err(())
        }
    }

    /// Try to disable the client
    pub(crate) fn try_disable(&self) -> Result<(), ()> {
        let was_enabled = self.enabled.swap(false, Ordering::Relaxed);
        if was_enabled {
            // Immediately stop the client when it was disabled
            // TODO: force the scheduler
            self.stop();
            Ok(())
        } else {
            Err(())
        }
    }

    #[inline]
    pub(crate) fn stats(&self) -> &dyn stats::Client {
        self.node.client_stats()
    }

    #[inline]
    pub(crate) async fn get_last_job(&self) -> Option<Arc<dyn job::Bitcoin>> {
        self.node.get_last_job().await
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.node.stop();
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

#[derive(Debug)]
pub struct Group {
    pub descriptor: GroupDescriptor,
    scheduler_client_handles: Mutex<Vec<scheduler::ClientHandle>>,
    /// All clients in the group must support the same amount of midstates
    midstate_count: usize,
}

impl Group {
    fn new(descriptor: GroupDescriptor, midstate_count: usize) -> Self {
        Self {
            descriptor,
            scheduler_client_handles: Mutex::new(vec![]),
            midstate_count,
        }
    }

    #[inline]
    pub async fn len(&self) -> usize {
        self.scheduler_client_handles.lock().await.len()
    }

    #[inline]
    pub async fn is_empty(&self) -> bool {
        self.scheduler_client_handles.lock().await.is_empty()
    }

    #[inline]
    pub async fn get_clients(&self) -> Vec<Arc<Handle>> {
        self.scheduler_client_handles
            .lock()
            .await
            .iter()
            .map(|scheduler_group_handle| scheduler_group_handle.client_handle.clone())
            .collect()
    }

    pub async fn push_client(&self, client_handle: Handle) -> Arc<Handle> {
        let midstate_count = self.midstate_count;
        let _ = client_handle.replace_engine_generator(Box::new(move |job| {
            Arc::new(work::engine::VersionRolling::new(job, midstate_count))
        }));
        let _ = client_handle.try_disable();

        let client_handle = Arc::new(client_handle);
        let scheduler_client_handle = scheduler::ClientHandle::new(client_handle.clone());
        self.scheduler_client_handles
            .lock()
            .await
            .push(scheduler_client_handle);

        if client_handle.descriptor.enable {
            client_handle
                .try_enable()
                .expect("BUG: client is already enabled");
        }

        client_handle
    }

    pub async fn remove_client_at(&self, index: usize) -> Result<Arc<Handle>, error::Client> {
        let mut scheduler_client_handles = self.scheduler_client_handles.lock().await;
        if index >= scheduler_client_handles.len() {
            Err(error::Client::Missing)
        } else {
            let client_handle = scheduler_client_handles.remove(index).client_handle;
            // Immediately disable client to force scheduler to select another client
            let _ = client_handle.try_disable();
            Ok(client_handle)
        }
    }

    /// Changes the position of a client within the group
    pub async fn move_client_to(
        &self,
        index_from: usize,
        index_to: usize,
    ) -> Result<Arc<Handle>, error::Client> {
        let mut scheduler_client_handles = self.scheduler_client_handles.lock().await;
        let len = scheduler_client_handles.len();
        if index_from >= len || index_to >= len {
            return Err(error::Client::Missing);
        }

        if index_from > index_to {
            *scheduler_client_handles = [
                &scheduler_client_handles[0..index_to],
                &scheduler_client_handles[index_from..index_from + 1],
                &scheduler_client_handles[index_to..index_from],
                &scheduler_client_handles[index_from + 1..],
            ]
            .concat();
        } else if index_from < index_to {
            *scheduler_client_handles = [
                &scheduler_client_handles[0..index_from],
                &scheduler_client_handles[index_from + 1..index_to + 1],
                &scheduler_client_handles[index_from..index_from + 1],
                &scheduler_client_handles[index_to + 1..],
            ]
            .concat();
        }

        Ok(scheduler_client_handles[index_to].client_handle.clone())
    }

    async fn find_client(&self, solution: &work::Solution) -> Option<Arc<Handle>> {
        self.scheduler_client_handles
            .lock()
            .await
            .iter()
            .find(|scheduler_client_handle| {
                scheduler_client_handle
                    .client_handle
                    .matching_solution(solution)
            })
            .map(|scheduler_client_handle| scheduler_client_handle.client_handle.clone())
    }
}

/// Keeps track of all active clients
pub struct GroupRegistry {
    list: Vec<scheduler::GroupHandle>,
    fixed_share_ratio_count: usize,
    total_fixed_share_ratio: f64,
}

impl GroupRegistry {
    pub fn new() -> Self {
        Self {
            list: vec![],
            fixed_share_ratio_count: 0,
            total_fixed_share_ratio: 0.0,
        }
    }

    #[inline]
    pub fn count(&self) -> usize {
        self.list.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    #[inline]
    fn iter(&self) -> slice::Iter<scheduler::GroupHandle> {
        self.list.iter()
    }

    #[inline]
    fn iter_mut(&mut self) -> slice::IterMut<scheduler::GroupHandle> {
        self.list.iter_mut()
    }

    /// Creates a new group that handles clients connected to pools that support `midstate_count`
    /// of midstates.
    /// TODO: once this functionality is available through the API, we should review arbitrary
    ///  recalculation of quotas
    pub fn create_group(
        &mut self,
        descriptor: GroupDescriptor,
        midstate_count: usize,
    ) -> Result<Arc<Group>, error::Client> {
        match descriptor.fixed_share_ratio {
            Some(fixed_share_ratio) => {
                if self.is_empty() {
                    Err(error::Client::OnlyFixedShareRatio)?;
                } else if self.total_fixed_share_ratio + fixed_share_ratio >= 1.0 {
                    Err(error::Client::FixedShareRatioOverflow)?;
                }
                self.fixed_share_ratio_count += 1;
                self.total_fixed_share_ratio += fixed_share_ratio;
            }
            None => {}
        }

        let group_handle = Arc::new(Group::new(descriptor, midstate_count));
        let scheduler_group_handle = scheduler::GroupHandle::new(group_handle.clone());
        self.list.push(scheduler_group_handle);
        self.recalculate_quotas(true);

        Ok(group_handle)
    }

    pub fn get_groups(&self) -> Vec<Arc<Group>> {
        self.list
            .iter()
            .map(|scheduler_group_handle| scheduler_group_handle.group_handle.clone())
            .collect()
    }

    pub fn get_group(&self, index: usize) -> Option<Arc<Group>> {
        self.list
            .get(index)
            .map(|scheduler_group_handle| scheduler_group_handle.group_handle.clone())
    }

    /// Find client which given solution is associated with
    async fn find_client(&self, solution: &work::Solution) -> Option<Arc<Handle>> {
        for scheduler_group_handle in &self.list {
            match scheduler_group_handle
                .group_handle
                .find_client(solution)
                .await
            {
                client_handle @ Some(_) => return client_handle,
                None => {}
            }
        }
        None
    }

    fn recalculate_quotas(&mut self, reset_generated_work: bool) {
        assert!(
            self.total_fixed_share_ratio < 1.0 && self.fixed_share_ratio_count < self.count(),
            "BUG: no share ratio left for common groups"
        );

        if self.is_empty() {
            return;
        }

        let common_groups = self.count() - self.fixed_share_ratio_count;
        let share_ratio = (1.0 - self.total_fixed_share_ratio) / common_groups as f64;

        // Update all groups with newly calculated share ratio.
        // Also reset generated work to prevent switching all future work to new group because
        // new group has zero shares and so maximal error.
        for mut scheduler_group_handle in self.list.iter_mut() {
            if reset_generated_work {
                scheduler_group_handle.reset_generated_work();
            }
            if !scheduler_group_handle.has_fixed_share_ratio() {
                scheduler_group_handle.share_ratio = share_ratio;
            }
        }
    }
}
