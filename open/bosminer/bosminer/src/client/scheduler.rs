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

#[derive(Debug)]
pub struct Handle {
    pub node: Arc<dyn node::Client>,
    engine_sender: Arc<work::EngineSender>,
    solution_sender: mpsc::UnboundedSender<work::Solution>,
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
}

/// Responsible for selecting and switching jobs
struct JobDispatcher {
    midstate_count: usize,
    engine_sender: Option<work::EngineSender>,
    active_client: Option<Arc<Handle>>,
    client_registry: Arc<Mutex<client::Registry>>,
}

impl JobDispatcher {
    async fn create_and_register_client<F, T>(
        &self,
        engine_sender: work::EngineSender,
        descriptor: Descriptor,
        create: F,
    ) -> client::Handle
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

        // update all clients with newly calculated percentage share
        for mut client in client_registry.iter_mut() {
            client.percentage_share = percentage_share;
        }

        let (solution_sender, solution_receiver) = mpsc::unbounded();

        let engine_sender = Arc::new(engine_sender);
        let job_solver = job::Solver::new(
            self.midstate_count,
            engine_sender.clone(),
            solution_receiver,
        );

        let client_handle = client::Handle::new(
            descriptor,
            create(job_solver),
            engine_sender,
            solution_sender,
            percentage_share,
        );

        client_registry.register_client(client_handle).clone()
    }

    pub async fn add_client<F, T>(&mut self, descriptor: Descriptor, create: F) -> client::Handle
    where
        T: node::Client + 'static,
        F: FnOnce(job::Solver) -> T,
    {
        let engine_sender = self
            .engine_sender
            .take()
            .unwrap_or_else(|| work::EngineSender::new(None));

        let client_handle =
            Self::create_and_register_client(self, engine_sender, descriptor, create).await;

        // when there is no active client then set current one
        self.active_client
            .get_or_insert_with(|| client_handle.scheduler_handle.clone());
        client_handle
    }

    fn switch_client(&mut self, next_client: Arc<Handle>) {
        let active_client = self
            .active_client
            .as_mut()
            .expect("BUG: missing active client");
        if &next_client != active_client {
            next_client
                .engine_sender
                .swap_sender(&active_client.engine_sender);
            *active_client = next_client;
        }
    }

    async fn select_client(&self, generated_work_delta: u64) -> Option<Arc<Handle>> {
        let mut client_registry = self.client_registry.lock().await;

        let mut total_generated_work = 0;
        for client in client_registry.iter_mut() {
            total_generated_work += client.update_generated_work();
        }

        let mut next_client = None;
        for client in client_registry.iter() {
            let client_generated_work = client.generated_work.count();
            let next_client_percentage_share = (client_generated_work + generated_work_delta)
                as f64
                / (total_generated_work + generated_work_delta) as f64;
            let next_error = (client.percentage_share - next_client_percentage_share).abs();
            match next_client {
                None => next_client = Some((client.scheduler_handle.clone(), next_error)),
                Some((_, min_error)) => {
                    if min_error >= next_error {
                        next_client = Some((client.scheduler_handle.clone(), next_error));
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
            dispatcher: Mutex::new(JobDispatcher {
                midstate_count,
                engine_sender: Some(engine_sender),
                active_client: None,
                client_registry,
            }),
        }
    }

    async fn lock_dispatcher(&self) -> MutexGuard<'_, JobDispatcher> {
        self.dispatcher.lock().await
    }

    async fn active_client(&self) -> Option<Arc<Handle>> {
        self.lock_dispatcher().await.active_client.clone()
    }

    /// Find client which given solution is associated with
    async fn find_client(&self, solution: &work::Solution) -> Option<Arc<Handle>> {
        self.client_registry
            .lock()
            .await
            .iter()
            .find(|client| client.scheduler_handle.matching_solution(solution))
            .map(|client| client.scheduler_handle.clone())
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

    pub async fn add_client<F, T>(&self, descriptor: Descriptor, create: F) -> client::Handle
    where
        T: node::Client + 'static,
        F: FnOnce(job::Solver) -> T,
    {
        self.lock_dispatcher()
            .await
            .add_client(descriptor, create)
            .await
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
