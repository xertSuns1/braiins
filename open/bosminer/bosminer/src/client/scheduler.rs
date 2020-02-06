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
use crate::error;
use crate::job;
use crate::node;
use crate::work;

use bosminer_config::client::Descriptor;

use futures::channel::mpsc;
use futures::lock::{Mutex, MutexGuard};
use ii_async_compat::{futures, tokio};
use tokio::time::delay_for;

use std::sync::Arc;
use std::time;

/// Private client handle with internal information which shouldn't be leaked
#[derive(Debug)]
pub struct Handle {
    pub client_handle: Arc<client::Handle>,
    generated_work: LocalGeneratedWork,
    percentage_share: f64,
}

impl Handle {
    pub fn new<T>(
        descriptor: Descriptor,
        client_node: T,
        engine_sender: Arc<work::EngineSender>,
        solution_sender: mpsc::UnboundedSender<work::Solution>,
        percentage_share: f64,
    ) -> Self
    where
        T: node::Client + 'static,
    {
        Self {
            client_handle: Arc::new(client::Handle::new::<T>(
                descriptor,
                client_node,
                engine_sender,
                solution_sender,
            )),
            generated_work: LocalGeneratedWork::new(),
            percentage_share,
        }
    }

    fn update_generated_work(&mut self) -> u64 {
        let global_generated_work = *self
            .client_handle
            .node
            .client_stats()
            .generated_work()
            .take_snapshot();
        self.generated_work.update(global_generated_work)
    }

    fn reset_generated_work(&mut self) {
        self.generated_work.reset();
    }
}

impl PartialEq for Handle {
    fn eq(&self, other: &Handle) -> bool {
        &self.client_handle == &other.client_handle
    }
}

/// Used for measuring generated work from global counter and allows the scheduler to arbitrarily
/// reset this counter
#[derive(Debug, Clone)]
pub struct LocalGeneratedWork {
    global_counter: u64,
    local_counter: u64,
}

impl LocalGeneratedWork {
    pub fn new() -> Self {
        Self {
            global_counter: Default::default(),
            local_counter: Default::default(),
        }
    }

    pub fn count(&self) -> u64 {
        self.local_counter
    }

    pub fn update(&mut self, global_counter: u64) -> u64 {
        assert!(
            global_counter >= self.global_counter,
            "generated work global counter must be monotonic"
        );

        let counter_delta = global_counter - self.global_counter;
        self.global_counter = global_counter;
        self.local_counter += counter_delta;

        self.local_counter
    }

    pub fn reset(&mut self) {
        self.local_counter = Default::default();
    }
}

enum ActiveClient {
    None(Arc<work::EngineSender>),
    Some(Arc<client::Handle>),
}

impl ActiveClient {
    #[inline]
    pub fn is_some(&self) -> bool {
        match *self {
            Self::Some(_) => true,
            Self::None(_) => false,
        }
    }

    #[inline]
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
    midstate_count: usize,
    active_client: ActiveClient,
    client_registry: Arc<Mutex<client::Registry>>,
}

impl JobDispatcher {
    fn new(
        midstate_count: usize,
        engine_sender: work::EngineSender,
        client_registry: Arc<Mutex<client::Registry>>,
    ) -> Self {
        Self {
            midstate_count,
            active_client: ActiveClient::None(Arc::new(engine_sender)),
            client_registry,
        }
    }

    /// Returns the registered `client::Handle` and its registration index
    async fn create_and_register_client<F, T>(
        &mut self,
        descriptor: Descriptor,
        create: F,
    ) -> (Arc<client::Handle>, usize)
    where
        T: node::Client + 'static,
        F: FnOnce(job::Solver) -> T,
    {
        let mut client_registry = self.client_registry.lock().await;

        let clients = client_registry.count();
        let percentage_share = if clients > 0 {
            1.0 / (clients + 1) as f64
        } else {
            1.0
        };

        // Update all clients with newly calculated percentage share.
        // Also reset generated work to prevent switching all future work to new client because
        // new client has zero shares and so maximal error.
        for mut client in client_registry.iter_mut() {
            client.reset_generated_work();
            client.percentage_share = percentage_share;
        }

        let (solution_sender, solution_receiver) = mpsc::unbounded();

        // Initially register new client without ability to send work
        let engine_sender = Arc::new(work::EngineSender::new(None));
        let job_solver = job::Solver::new(
            self.midstate_count,
            engine_sender.clone(),
            solution_receiver,
        );

        let enable_client = descriptor.enable;
        let scheduler_handle = Handle::new(
            descriptor,
            create(job_solver),
            engine_sender,
            solution_sender,
            percentage_share,
        );

        let (scheduler_handle, client_idx) = client_registry.register_client(scheduler_handle);

        if enable_client {
            scheduler_handle
                .client_handle
                .try_enable()
                .expect("BUG: client is already enabled");
        }

        (scheduler_handle.client_handle.clone(), client_idx)
    }

    async fn add_client<F, T>(
        &mut self,
        descriptor: Descriptor,
        create: F,
    ) -> (Arc<client::Handle>, usize)
    where
        T: node::Client + 'static,
        F: FnOnce(job::Solver) -> T,
    {
        let (client_handle, client_idx) = self.create_and_register_client(descriptor, create).await;

        // When there is no active client then set current one
        if self.active_client.is_none() {
            self.switch_client(client_handle.clone());
        }

        (client_handle, client_idx)
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
        let mut client_registry = self.client_registry.lock().await;

        let mut total_generated_work = 0;
        for client in client_registry.iter_mut() {
            total_generated_work += client.update_generated_work();
        }

        let mut next_client = None;
        for scheduler_handle in client_registry.iter() {
            let client_generated_work = scheduler_handle.generated_work.count();
            let next_client_percentage_share = (client_generated_work + generated_work_delta)
                as f64
                / (total_generated_work + generated_work_delta) as f64;
            let next_error =
                (scheduler_handle.percentage_share - next_client_percentage_share).abs();
            match next_client {
                None => next_client = Some((scheduler_handle.client_handle.clone(), next_error)),
                Some((_, min_error)) => {
                    if min_error >= next_error {
                        next_client = Some((scheduler_handle.client_handle.clone(), next_error));
                    }
                }
            }
        }
        next_client.map(|(next_client, _)| next_client)
    }

    async fn schedule(&mut self, generated_work_delta: u64) {
        let next_client = self.select_client(generated_work_delta).await;

        if let Some(next_client) = next_client {
            self.switch_client(next_client);
        }
    }
}

/// Responsible for dispatching new clients and planning generated jobs to be solved
pub struct JobExecutor {
    frontend: Arc<crate::Frontend>,
    client_registry: Arc<Mutex<client::Registry>>,
    dispatcher: Mutex<JobDispatcher>,
}

impl JobExecutor {
    const SCHEDULE_INTERVAL: time::Duration = time::Duration::from_secs(1);

    pub fn new(
        midstate_count: usize,
        frontend: Arc<crate::Frontend>,
        engine_sender: work::EngineSender,
        client_registry: Arc<Mutex<client::Registry>>,
    ) -> Self {
        Self {
            frontend,
            client_registry: client_registry.clone(),
            dispatcher: Mutex::new(JobDispatcher::new(
                midstate_count,
                engine_sender,
                client_registry,
            )),
        }
    }

    async fn lock_dispatcher(&self) -> MutexGuard<'_, JobDispatcher> {
        self.dispatcher.lock().await
    }

    async fn active_client(&self) -> Option<Arc<client::Handle>> {
        self.lock_dispatcher().await.active_client.get_client()
    }

    /// Find client which given solution is associated with
    async fn find_client(&self, solution: &work::Solution) -> Option<Arc<client::Handle>> {
        self.client_registry
            .lock()
            .await
            .iter()
            .find(|scheduler_handle| scheduler_handle.client_handle.matching_solution(solution))
            .map(|scheduler_handle| scheduler_handle.client_handle.clone())
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

    pub async fn add_client<F, T>(
        &self,
        descriptor: Descriptor,
        create: F,
    ) -> (Arc<client::Handle>, usize)
    where
        T: node::Client + 'static,
        F: FnOnce(job::Solver) -> T,
    {
        self.lock_dispatcher()
            .await
            .add_client(descriptor, create)
            .await
    }

    pub async fn swap_clients(
        &self,
        a: usize,
        b: usize,
    ) -> Result<(Arc<client::Handle>, Arc<client::Handle>), error::Client> {
        self.client_registry.lock().await.swap_clients(a, b)
        // TODO: force scheduler
    }

    pub async fn run(self: Arc<Self>) {
        loop {
            let last_generated_work = self.frontend.get_generated_work();

            delay_for(Self::SCHEDULE_INTERVAL).await;

            // determine how much work has been generated from last run
            let generated_work_delta = self.frontend.get_generated_work() - last_generated_work;
            if generated_work_delta == 0 {
                // when no work has been generated then keep running job unchanged
                continue;
            }

            self.lock_dispatcher()
                .await
                .schedule(generated_work_delta)
                .await;
        }
    }
}
