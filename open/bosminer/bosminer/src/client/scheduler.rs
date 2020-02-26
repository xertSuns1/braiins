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

use crate::client;
use crate::work;

use futures::channel::mpsc;
use futures::lock::{Mutex, MutexGuard};
use ii_async_compat::{futures, tokio};
use tokio::time::delay_for;

use std::sync::Arc;
use std::time;

/// This struct cannot be shared and it is possible to use mutable references. However, the
/// client handle is shared object with interior mutability scheduler::ClientHandle. It solves
/// many synchronization problems.
#[derive(Debug, Clone)]
pub struct ClientHandle {
    pub client_handle: Arc<client::Handle>,
    last_generated_work: u64,
}

impl ClientHandle {
    pub fn new(client_handle: Arc<client::Handle>) -> Self {
        Self {
            last_generated_work: Self::get_generated_work(&client_handle),
            client_handle,
        }
    }

    #[inline]
    fn is_running(&self) -> bool {
        self.client_handle.is_running()
    }

    #[inline]
    fn try_start(&self) -> Result<(), ()> {
        if self.client_handle.is_enabled() {
            self.client_handle.start();
            Ok(())
        } else {
            Err(())
        }
    }

    #[inline]
    fn try_delayed_stop(&self) -> Result<(), ()> {
        // TODO: Implement delay before actual stopping
        if self.client_handle.is_enabled() {
            self.client_handle.stop();
            Ok(())
        } else {
            Err(())
        }
    }

    fn get_generated_work(client_handle: &Arc<client::Handle>) -> u64 {
        *client_handle
            .node
            .client_stats()
            .generated_work()
            .take_snapshot()
    }

    pub fn get_delta_and_update_generated_work(&mut self) -> u64 {
        let next_generated_work = Self::get_generated_work(&self.client_handle);
        assert!(
            next_generated_work >= self.last_generated_work,
            "generated work must be monotonic"
        );

        let delta = next_generated_work - self.last_generated_work;
        self.last_generated_work = next_generated_work;
        delta
    }
}

impl PartialEq for ClientHandle {
    fn eq(&self, other: &ClientHandle) -> bool {
        &self.client_handle == &other.client_handle
    }
}

/// Private client handle with internal information which shouldn't be leaked
#[derive(Debug, Clone)]
pub struct GroupHandle {
    pub group_handle: Arc<client::Group>,
    active_client: Option<Arc<client::Handle>>,
    generated_work: u64,
    /// Current ratio of hashrate that this group has been allocated to. This number
    /// changes based on newly added/removed groups.
    pub share_ratio: f64,
}

impl GroupHandle {
    pub fn new(group_handle: Arc<client::Group>) -> Self {
        Self {
            active_client: None,
            generated_work: 0,
            share_ratio: group_handle
                .descriptor
                .get_fixed_share_ratio()
                .unwrap_or_default(),
            group_handle,
        }
    }

    #[inline]
    pub fn has_fixed_share_ratio(&self) -> bool {
        self.group_handle
            .descriptor
            .get_fixed_share_ratio()
            .is_some()
    }

    async fn update_status(&mut self) {
        let mut scheduler_client_handles = self.group_handle.scheduler_client_handles.lock().await;
        let mut generated_work_delta = 0;

        self.active_client = None;
        for scheduler_client_handle in scheduler_client_handles.iter_mut() {
            generated_work_delta += scheduler_client_handle.get_delta_and_update_generated_work();
            match self.active_client {
                None => {
                    if scheduler_client_handle.is_running() {
                        self.active_client = Some(scheduler_client_handle.client_handle.clone());
                    } else {
                        let _ = scheduler_client_handle.try_start();
                    }
                }
                Some(_) => {
                    let _ = scheduler_client_handle.try_delayed_stop();
                }
            }
        }

        self.generated_work += generated_work_delta;
    }

    #[inline]
    pub fn reset_generated_work(&mut self) {
        self.generated_work = 0;
    }
}

enum ActiveClient {
    None(Arc<work::EngineSender>),
    Some(Arc<client::Handle>),
}

impl ActiveClient {
    #[inline]
    #[allow(dead_code)]
    pub fn is_some(&self) -> bool {
        match *self {
            Self::Some(_) => true,
            Self::None(_) => false,
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn is_none(&self) -> bool {
        !self.is_some()
    }

    #[inline]
    pub fn get_client(&self) -> Option<Arc<client::Handle>> {
        match self {
            Self::Some(client) => Some(client.clone()),
            Self::None(_) => None,
        }
    }

    #[inline]
    pub fn get_engine_sender(&self) -> &Arc<work::EngineSender> {
        match self {
            Self::Some(client) => &client.engine_sender,
            Self::None(engine_sender) => engine_sender,
        }
    }
}

impl PartialEq<Arc<client::Handle>> for ActiveClient {
    fn eq(&self, other: &Arc<client::Handle>) -> bool {
        match self {
            Self::Some(client) => client == other,
            Self::None(_) => false,
        }
    }
}

/// Responsible for selecting and switching jobs
struct JobDispatcher {
    active_client: ActiveClient,
    group_registry: Arc<Mutex<client::GroupRegistry>>,
}

impl JobDispatcher {
    fn new(
        engine_sender: work::EngineSender,
        group_registry: Arc<Mutex<client::GroupRegistry>>,
    ) -> Self {
        Self {
            active_client: ActiveClient::None(Arc::new(engine_sender)),
            group_registry,
        }
    }

    fn switch_client<T>(&mut self, next_client: T)
    where
        T: Into<Option<Arc<client::Handle>>>,
    {
        match next_client.into() {
            Some(next_client) => {
                if self.active_client != next_client {
                    next_client
                        .engine_sender
                        .swap_sender(self.active_client.get_engine_sender());
                    self.active_client = ActiveClient::Some(next_client);
                }
            }
            None => match &self.active_client {
                ActiveClient::Some(prev_client) => {
                    self.active_client = ActiveClient::None(prev_client.engine_sender.clone());
                }
                ActiveClient::None(_) => {}
            },
        }
    }

    async fn select_client(&self, generated_work_delta: u64) -> Option<Arc<client::Handle>> {
        let mut group_registry = self.group_registry.lock().await;
        if group_registry.is_empty() {
            return None;
        }

        let mut total_generated_work = 0;
        for scheduler_group_handle in group_registry.iter_mut() {
            scheduler_group_handle.update_status().await;
            total_generated_work += scheduler_group_handle.generated_work;
        }

        let mut next_client = None;
        for scheduler_group_handle in group_registry.iter() {
            let group_generated_work = scheduler_group_handle.generated_work;
            let next_group_share_ratio = (group_generated_work + generated_work_delta) as f64
                / (total_generated_work + generated_work_delta) as f64;
            let next_error = (scheduler_group_handle.share_ratio - next_group_share_ratio).abs();
            if let Some(active_client) = scheduler_group_handle.active_client.as_ref().cloned() {
                match next_client {
                    None => next_client = Some((active_client, next_error)),
                    Some((_, min_error)) => {
                        if min_error >= next_error {
                            next_client = Some((active_client, next_error));
                        }
                    }
                }
            }
        }
        next_client.map(|(next_client, _)| next_client)
    }

    async fn schedule(&mut self, generated_work_delta: u64) {
        match &self.active_client {
            ActiveClient::Some(client_handle) => {
                if generated_work_delta == 0 && client_handle.is_running() {
                    // When some client is active and no work has been generated then do nothing
                    return;
                }
            }
            _ => {}
        }
        if let Some(next_client) = self.select_client(generated_work_delta).await {
            self.switch_client(next_client);
        }
    }
}

/// Responsible for dispatching new clients and planning generated jobs to be solved
pub struct JobExecutor {
    frontend: Arc<crate::Frontend>,
    group_registry: Arc<Mutex<client::GroupRegistry>>,
    dispatcher: Mutex<JobDispatcher>,
}

impl JobExecutor {
    const SCHEDULE_INTERVAL: time::Duration = time::Duration::from_secs(1);

    pub fn new(
        frontend: Arc<crate::Frontend>,
        engine_sender: work::EngineSender,
        group_registry: Arc<Mutex<client::GroupRegistry>>,
    ) -> Self {
        Self {
            frontend,
            group_registry: group_registry.clone(),
            dispatcher: Mutex::new(JobDispatcher::new(engine_sender, group_registry)),
        }
    }

    async fn lock_dispatcher(&self) -> MutexGuard<'_, JobDispatcher> {
        self.dispatcher.lock().await
    }

    async fn active_client(&self) -> Option<Arc<client::Handle>> {
        self.lock_dispatcher().await.active_client.get_client()
    }

    #[inline]
    async fn find_client(&self, solution: &work::Solution) -> Option<Arc<client::Handle>> {
        self.group_registry.lock().await.find_client(solution).await
    }

    pub async fn get_solution_sender(
        &self,
        solution: &work::Solution,
    ) -> Option<mpsc::UnboundedSender<work::Solution>> {
        let active_client = self.active_client().await;

        // solution receiver is probably active client which is work generated from
        let mut client = active_client.filter(|client| client.matching_solution(solution));
        // search client registry when active client is not matching destination sender
        if client.is_none() {
            client = self.find_client(&solution).await
        }
        // return associated solution sender when matching client is found
        client.map(|client| client.solution_sender.clone())
    }

    pub async fn run(self: Arc<Self>) {
        loop {
            let last_generated_work = self.frontend.get_generated_work();

            // TODO: Interrupt waiting whenever the client state has changed
            delay_for(Self::SCHEDULE_INTERVAL).await;

            // Determine how much work has been generated from last run
            let generated_work_delta = self.frontend.get_generated_work() - last_generated_work;

            self.lock_dispatcher()
                .await
                .schedule(generated_work_delta)
                .await;
        }
    }
}
