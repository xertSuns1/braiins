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

use futures::channel::mpsc;
use futures::lock::{Mutex, MutexGuard};
use ii_async_compat::{futures, tokio};
use tokio::time::delay_for;

use std::sync::Arc;
use std::time;

#[derive(Debug)]
pub struct ClientHandle {
    pub handle: Arc<client::Handle>,
    generated_work: LocalGeneratedWork,
    percentage_share: f64,
}

impl ClientHandle {
    pub fn new<T>(
        client: T,
        engine_sender: Arc<work::EngineSender>,
        solution_sender: mpsc::UnboundedSender<work::Solution>,
        percentage_share: f64,
    ) -> Self
    where
        T: node::Client + 'static,
    {
        Self {
            handle: Arc::new(client::Handle::new::<T>(
                client,
                engine_sender,
                solution_sender,
            )),
            generated_work: LocalGeneratedWork::new(),
            percentage_share,
        }
    }

    pub fn update_generated_work(&mut self) -> u64 {
        let global_generated_work = *self
            .handle
            .node
            .client_stats()
            .generated_work()
            .take_snapshot();
        self.generated_work.update(global_generated_work)
    }
}

impl PartialEq for ClientHandle {
    fn eq(&self, other: &ClientHandle) -> bool {
        &self.handle == &other.handle
    }
}

/// Used for measuring generated work from global counter and allows the scheduler to arbitrarily
/// reset this counter
#[derive(Debug)]
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
    active_client: Option<Arc<client::Handle>>,
    client_registry: Arc<Mutex<client::Registry>>,
}

impl JobDispatcher {
    async fn create_and_register_client<F, T>(
        &self,
        engine_sender: work::EngineSender,
        create: F,
    ) -> Arc<client::Handle>
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

        let client_handle = ClientHandle::new(
            create(job_solver),
            engine_sender,
            solution_sender,
            percentage_share,
        );

        client_registry
            .register_client(client_handle)
            .handle
            .clone()
    }

    pub async fn add_client<F, T>(&mut self, create: F) -> Arc<dyn node::Client>
    where
        T: node::Client + 'static,
        F: FnOnce(job::Solver) -> T,
    {
        let engine_sender = self
            .engine_sender
            .take()
            .unwrap_or_else(|| work::EngineSender::new(None));

        let client_handle = Self::create_and_register_client(self, engine_sender, create).await;

        let client = client_handle.node.clone();
        // when there is no active client then set current one
        self.active_client.get_or_insert(client_handle);
        client
    }

    fn switch_client(&mut self, next_client: Arc<client::Handle>) {
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

    async fn select_client(&self, generated_work_delta: u64) -> Option<Arc<client::Handle>> {
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
                None => next_client = Some((client.handle.clone(), next_error)),
                Some((_, min_error)) => {
                    if min_error >= next_error {
                        next_client = Some((client.handle.clone(), next_error));
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

    pub async fn active_client(&self) -> Option<Arc<client::Handle>> {
        self.lock_dispatcher().await.active_client.clone()
    }

    pub async fn add_client<F, T>(&self, create: F) -> Arc<dyn node::Client>
    where
        T: node::Client + 'static,
        F: FnOnce(job::Solver) -> T,
    {
        self.lock_dispatcher().await.add_client(create).await
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
