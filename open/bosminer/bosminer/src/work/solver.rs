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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time;

type WorkSolverPath = Vec<Arc<dyn node::WorkSolver>>;

/// Compound object that is supposed to be sent down to the mining backend that can in turn solve
/// any generated work and submit solutions.
pub struct Solver {
    /// Flag indicating that this is work hub (special case of work solver)
    hub: AtomicBool,
    /// Unique path describing internal hierarchy of backend solvers
    path: WorkSolverPath,
    /// Shared engine receiver needed for creating `Generator`
    engine_receiver: EngineReceiver,
    /// Solution submission channel for the underlying mining backend
    solution_sender: SolutionSender,
    hierarchy_builder: Arc<dyn backend::HierarchyBuilder>,
}

impl Solver {
    pub fn split(self) -> (Generator, SolutionSender) {
        assert_eq!(
            self.hub.load(Ordering::Relaxed),
            false,
            "the work hub cannot be split"
        );
        (
            Generator::new(self.engine_receiver, self.path),
            self.solution_sender,
        )
    }

    /// Construct new work solver from engine receiver and associated channel to send the results
    pub(crate) async fn create_root<T: node::WorkSolver + 'static>(
        hierarchy_builder: Arc<dyn backend::HierarchyBuilder>,
        node: Arc<T>,
        engine_receiver: EngineReceiver,
        solution_queue_tx: mpsc::UnboundedSender<Solution>,
    ) -> Self {
        hierarchy_builder.add_root(node.clone()).await;

        Self {
            hub: AtomicBool::new(false),
            path: vec![node],
            engine_receiver,
            solution_sender: SolutionSender(solution_queue_tx),
            hierarchy_builder,
        }
    }

    /// Create another solver based on previous one.
    /// It provides generic way how to describe hierarchy in various backends.
    /// Each solver has unique path described by generic node info.
    pub async fn branch<T: node::WorkSolver + 'static>(&self, node: Arc<T>) -> Self {
        // mark work solver which new one is branched from as a work hub
        let first_child = !self.hub.compare_and_swap(false, true, Ordering::Relaxed);
        self.hierarchy_builder
            .branch(
                first_child,
                self.path
                    .last()
                    .expect("BUG: empty path in `work::Solver`")
                    .clone(),
                node.clone(),
            )
            .await;

        let mut path = self.path.clone();
        path.push(node.clone());
        Self {
            hub: AtomicBool::new(false),
            path,
            engine_receiver: self.engine_receiver.clone(),
            solution_sender: self.solution_sender.clone(),
            hierarchy_builder: self.hierarchy_builder.clone(),
        }
    }
}

/// Generator is responsible for accepting a `WorkEngine` and draining as much
/// `MiningWork` as possible from it.
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
            let now = time::SystemTime::now();
            for node in self.path.iter() {
                // Arc does not support dynamic casting to trait bounds so there must be used
                // another Arc indirection with implemented `node::Info` trait.
                // This blanket implementation can be found in the module `crate::node`:
                // impl<T: ?Sized + Info> Info for Arc<T> {}
                work.path.push(Arc::new(node.clone()));
                node.work_solver_stats().last_work_time().touch(now).await;
            }
            return Some(work);
        }
    }
}

/// This struct is to be passed to the underlying mining backend. It allows submission of
/// `work::Solution`
#[derive(Clone)]
pub struct SolutionSender(mpsc::UnboundedSender<Solution>);

impl SolutionSender {
    pub fn send(&self, solution: Solution) {
        self.0
            .unbounded_send(solution)
            .expect("solution queue send failed");
    }
}
