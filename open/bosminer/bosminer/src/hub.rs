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

//! Top level builder for `job::Solver` and `work::Solver` intended to be used when instantiating
//! the full miner

use ii_logging::macros::*;

use crate::backend;
use crate::client;
use crate::hal;
use crate::job;
use crate::node;
use crate::shutdown;
use crate::work;

use futures::channel::mpsc;
use futures::lock::Mutex;
use ii_async_compat::futures;

use std::sync::Arc;

/// Handle external events. Currently it is used only wor handling exhausted work from work engine.
/// It usually signals some serious problem in backend.
struct EventHandler;

impl work::ExhaustedHandler for EventHandler {
    fn handle_exhausted(&self, _engine: work::DynEngine) {
        warn!("No more work available for current job!");
    }
}

pub struct Core {
    frontend_info: node::DynInfo,
    engine_sender: Mutex<Option<work::EngineSender>>,
    engine_receiver: work::EngineReceiver,
    solution_sender: mpsc::UnboundedSender<work::Solution>,
    solution_receiver: Mutex<Option<mpsc::UnboundedReceiver<work::Solution>>>,
    backend_registry: Arc<backend::Registry>,
    /// Registry of clients that are able to supply new jobs for mining
    client_registry: client::Registry,
}

impl Core {
    pub fn new(frontend_info: node::DynInfo) -> Self {
        let (engine_sender, engine_receiver) = work::engine_channel(EventHandler);
        let (solution_sender, solution_receiver) = mpsc::unbounded();

        Self {
            frontend_info,
            engine_sender: Mutex::new(Some(engine_sender)),
            engine_receiver,
            solution_sender,
            solution_receiver: Mutex::new(Some(solution_receiver)),
            backend_registry: Arc::new(backend::Registry::new()),
            client_registry: client::Registry::new(),
        }
    }

    pub async fn add_backend<T: hal::Backend>(&self, backend: T) {
        let backend = Arc::new(backend);
        let (shutdown_sender, _shutdown_receiver) = shutdown::channel();

        let work_solver_builder = work::SolverBuilder::create_root(
            self.backend_registry.clone(),
            backend.clone(),
            self.engine_receiver.clone(),
            self.solution_sender.clone(),
        )
        .await;

        // immediately start HW backend for selected target
        backend.run(work_solver_builder, shutdown_sender);
    }

    pub async fn add_client<F, T>(&self, create: F) -> Arc<dyn node::Client>
    where
        T: node::Client + 'static,
        F: FnOnce(job::Solver) -> T,
    {
        let job_solver = job::Solver::new(
            self.frontend_info.clone(),
            self.engine_sender
                .lock()
                .await
                .take()
                .expect("BUG: missing engine sender"),
            self.solution_receiver
                .lock()
                .await
                .take()
                .expect("BUG: missing solution receiver"),
        );

        let client = Arc::new(create(job_solver));
        self.client_registry.register_client(client.clone()).await;

        // convert it to dynamic object
        client as Arc<dyn node::Client>
    }

    #[inline]
    pub async fn get_root_hub(&self) -> Option<Arc<dyn node::WorkSolver>> {
        self.backend_registry.get_root_hub().await
    }

    #[inline]
    pub async fn get_work_hubs(&self) -> Vec<Arc<dyn node::WorkSolver>> {
        self.backend_registry.get_work_hubs().await
    }

    #[inline]
    pub async fn get_work_solvers(&self) -> Vec<Arc<dyn node::WorkSolver>> {
        self.backend_registry.get_work_solvers().await
    }

    #[inline]
    pub async fn get_clients(&self) -> Vec<Arc<dyn node::Client>> {
        self.client_registry.get_clients().await
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::test_utils;

    use ii_async_compat::tokio;
    use std::sync::Arc;

    /// Create job solver for frontend (pool) and work solver builder for backend (as we expect a
    /// hierarchical structure in backends)
    /// `backend_work_solver` is the root of the work solver hierarchy
    async fn build_solvers<T: node::WorkSolver + 'static>(
        frontend_info: node::DynInfo,
        backend_work_solver: Arc<T>,
    ) -> (job::Solver, work::SolverBuilder) {
        let (engine_sender, engine_receiver) = work::engine_channel(EventHandler);
        let (solution_sender, solution_receiver) = mpsc::unbounded();
        (
            job::Solver::new(frontend_info, engine_sender, solution_receiver),
            work::SolverBuilder::create_root(
                Arc::new(backend::Registry::new()),
                backend_work_solver,
                engine_receiver,
                solution_sender,
            )
            .await,
        )
    }

    /// This test verifies the whole lifecycle of a mining job, its transformation into work
    /// and also collection of the solution via solution receiver. No actual mining takes place
    /// in the test
    #[tokio::test]
    async fn test_solvers_connection() {
        let (job_solver, work_solver) = build_solvers(
            Arc::new(test_utils::TestNode::new()),
            Arc::new(test_utils::TestWorkSolver::new()),
        )
        .await;

        let (mut job_sender, mut solution_receiver) = job_solver.split();
        let (mut work_generator, solution_sender) = work_solver.split();

        // default target is be set to difficulty 1 so all solution should pass
        for block in test_utils::TEST_BLOCKS.iter() {
            let job = Arc::new(*block);

            // send prepared testing block to job solver
            job_sender.send(job);
            // work generator receives this job and prepares work from it
            let work = work_generator.generate().await.unwrap();
            // initial value for version rolling is 0 so midstate should match with expected one
            assert_eq!(block.midstate, work.midstates[0].state);
            // test block has automatic conversion into work solution
            solution_sender.send(block.into());
            // this solution should pass through job solver
            let solution = solution_receiver.receive().await.unwrap();
            // check if the solution is equal to expected one
            assert_eq!(block.nonce, solution.nonce());
            let original_job: &test_utils::TestBlock = solution.job();
            // the job should also match with original one
            // job solver does not returns Arc so the comparison is done by its hashes
            assert_eq!(block.hash, original_job.hash);
        }

        // work generator still works even if all job solvers are dropped
        drop(job_sender);
        drop(solution_receiver);
        assert!(work_generator.generate().await.is_some());
    }
}
