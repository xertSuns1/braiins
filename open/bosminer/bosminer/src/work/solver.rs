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

use super::*;
use crate::backend;
use crate::node;

use futures::channel::mpsc;
use ii_async_compat::futures;

use std::sync::Arc;
use std::time;

type WorkSolverPath = Vec<Arc<dyn node::WorkSolver>>;

enum NodeType<T> {
    Base(T),
    WorkHub(T),
}

/// Compound object that is supposed to be sent down to the mining backend for building hierarchy
/// of work solvers and work hubs (special case of solver which only routes work to its child nodes
/// and is useful for statistics aggregation and group control). Work solvers can be in turn split
/// to `work::Generator` and `work::SolutionSender` that can solve any generated work and submit
/// its solutions.
pub struct SolverBuilder<T> {
    node: NodeType<Arc<T>>,
    /// Unique path describing internal hierarchy of backend solvers
    path: WorkSolverPath,
    /// Shared engine receiver needed for creating `Generator`
    engine_receiver: EngineReceiver,
    /// Solution submission channel for the underlying mining backend
    solution_sender: SolutionSender,
    /// Custom hierarchy builder object driven by `SolverBuilder`
    hierarchy_builder: Arc<dyn backend::HierarchyBuilder>,
}

impl<T> SolverBuilder<T>
where
    T: node::WorkSolver + 'static,
{
    pub fn new(
        base_work_solver: Arc<T>,
        hierarchy_builder: Arc<dyn backend::HierarchyBuilder>,
        engine_receiver: EngineReceiver,
        solution_sender: mpsc::UnboundedSender<Solution>,
    ) -> Self {
        Self {
            node: NodeType::Base(base_work_solver),
            path: vec![],
            engine_receiver,
            solution_sender: SolutionSender(solution_sender),
            hierarchy_builder,
        }
    }

    #[inline]
    pub fn into_node(self) -> Arc<T> {
        match self.node {
            NodeType::Base(_) => panic!("cannot convert base work solver to dynamic node"),
            NodeType::WorkHub(node) => node,
        }
    }

    pub fn get_path(&self) -> WorkSolverPath {
        match &self.node {
            NodeType::Base(base) => vec![base.clone()],
            NodeType::WorkHub(work_hub) => {
                let mut path = self.path.clone();
                path.push(work_hub.clone());
                path
            }
        }
    }

    async fn call_hierarchy_builder(&self, node: node::WorkSolverType<Arc<dyn node::WorkSolver>>) {
        match &self.node {
            NodeType::Base(_) => {
                self.hierarchy_builder.add_root(node).await;
            }
            NodeType::WorkHub(work_hub) => {
                self.hierarchy_builder.branch(work_hub.clone(), node);
            }
        };
    }

    pub async fn create_work_hub<F, U>(&self, create: F) -> SolverBuilder<U>
    where
        U: node::WorkSolver + 'static,
        F: FnOnce() -> U,
    {
        let work_hub = Arc::new(create());
        self.call_hierarchy_builder(node::WorkSolverType::WorkHub(work_hub.clone()))
            .await;

        SolverBuilder {
            node: NodeType::WorkHub(work_hub),
            path: self.get_path(),
            engine_receiver: self.engine_receiver.clone(),
            solution_sender: self.solution_sender.clone(),
            hierarchy_builder: self.hierarchy_builder.clone(),
        }
    }

    pub async fn create_work_solver<F, U>(&self, create: F) -> Arc<U>
    where
        U: node::WorkSolver + 'static,
        F: FnOnce(Generator, SolutionSender) -> U,
    {
        let path = self.get_path();
        let work_generator = Generator::new(self.engine_receiver.clone(), path);
        let solution_sender = self.solution_sender.clone();

        let work_solver = Arc::new(create(work_generator, solution_sender));
        self.call_hierarchy_builder(node::WorkSolverType::WorkSolver(work_solver.clone()))
            .await;

        work_solver
    }
}

/// Generator is responsible for accepting a `WorkEngine` and draining as much
/// `MiningWork` as possible from it.
#[derive(Debug, Clone)]
pub struct Generator {
    /// Unique path describing internal hierarchy of backend solvers
    path: WorkSolverPath,
    /// Source of trait objects that implement `WorkEngine` interface
    engine_receiver: EngineReceiver,
}

impl Generator {
    pub fn new(engine_receiver: EngineReceiver, path: WorkSolverPath) -> Self {
        Self {
            path,
            engine_receiver,
        }
    }

    /// Loops until new work is available or no more `WorkEngines` are supplied (signals
    /// Generator shutdown)
    pub async fn generate(&mut self) -> Option<Assignment> {
        loop {
            let engine = match self.engine_receiver.get_engine().await {
                // end of stream
                None => return None,
                Some(value) => value,
            };
            // try to generate new work from engine
            let mut work = match engine.next_work() {
                // one or more competing work engines are exhausted
                // try to gen new work engine
                // NOTE: this can happen simultaneously for multiple parallel generators because
                // only one can win the last work and so there should not be included any logging
                LoopState::Exhausted => continue,
                // consecutive call of work engine may return new work
                LoopState::Continue(value) => value,
                // tha last work is returned from work engine (the work is exhausted)
                LoopState::Break(value) => {
                    // inform about this event
                    self.engine_receiver.handle_exhausted(engine);
                    value
                }
            };
            // determine how much work has been generated for current work assignment
            let work_amount = work.generated_work_amount() as u64;
            // account generated work on the client side
            work.origin()
                .client_stats()
                .generated_work()
                .add(work_amount);

            // account generated work in all work solvers in the path
            let now = time::SystemTime::now();
            for node in self.path.iter() {
                let work_solver_stats = node.work_solver_stats();
                // Arc does not support dynamic casting to trait bounds so there must be used
                // another Arc indirection with implemented `node::Info` trait.
                // This blanket implementation can be found in the module `crate::node`:
                // impl<T: ?Sized + Info> Info for Arc<T> {}
                work.path.push(Arc::new(node.clone()));
                work_solver_stats.generated_work().add(work_amount);
                work_solver_stats.last_work_time().touch(now).await;
            }
            return Some(work);
        }
    }
}

/// This struct is to be passed to the underlying mining backend. It allows submission of
/// `work::Solution`
#[derive(Debug, Clone)]
pub struct SolutionSender(mpsc::UnboundedSender<Solution>);

impl SolutionSender {
    pub fn send(&self, solution: Solution) {
        self.0
            .unbounded_send(solution)
            .expect("solution queue send failed");
    }
}
