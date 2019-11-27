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
use ii_async_compat::futures;

use std::sync::Arc;

struct JobExecutorInner {
    engine_sender: Option<work::EngineSender>,
    active_client: Option<Arc<client::Handle>>,
    clients_registry: Arc<client::Registry>,
}

pub struct JobExecutor {
    inner: Mutex<JobExecutorInner>,
}

impl JobExecutor {
    pub fn new(engine_sender: work::EngineSender, clients_registry: Arc<client::Registry>) -> Self {
        Self {
            inner: Mutex::new(JobExecutorInner {
                engine_sender: Some(engine_sender),
                active_client: None,
                clients_registry,
            }),
        }
    }

    async fn lock_inner(&self) -> MutexGuard<'_, JobExecutorInner> {
        self.inner.lock().await
    }

    pub async fn active_client(&self) -> Option<Arc<client::Handle>> {
        self.lock_inner().await.active_client.clone()
    }

    pub async fn clients_registry(&self) -> Arc<client::Registry> {
        self.lock_inner().await.clients_registry.clone()
    }

    pub async fn add_client<F, T>(&self, create: F) -> Arc<dyn node::Client>
    where
        T: node::Client + 'static,
        F: FnOnce(job::Solver) -> T,
    {
        let mut job_executor = self.lock_inner().await;

        let engine_sender = job_executor
            .engine_sender
            .take()
            .unwrap_or_else(|| work::EngineSender::new(None));
        let (solution_sender, solution_receiver) = mpsc::unbounded();

        let engine_sender = Arc::new(engine_sender);
        let job_solver = job::Solver::new(engine_sender.clone(), solution_receiver);

        let client_handle = client::Handle::new(create(job_solver), engine_sender, solution_sender);
        let client = client_handle.node.clone();

        let client_handle = job_executor
            .clients_registry
            .register_client(client_handle)
            .await;
        // when there is no active client then set current one
        job_executor.active_client.get_or_insert(client_handle);

        client
    }

    pub async fn run(self: Arc<Self>) {}
}
