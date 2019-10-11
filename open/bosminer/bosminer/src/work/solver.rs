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
use crate::node;

use futures::channel::mpsc;
use ii_async_compat::futures;

/// Compound object that is supposed to be sent down to the mining backend that can in turn solve
/// any generated work and submit solutions.
pub struct Solver {
    /// Unique path describing internal hierarchy of backend solvers
    path: node::Path,
    /// Shared engine receiver needed for creating `Generator`
    engine_receiver: EngineReceiver,
    /// Solution submission channel for the underlying mining backend
    solution_sender: SolutionSender,
}

impl Solver {
    pub fn split(self) -> (Generator, SolutionSender) {
        (
            Generator::new(self.engine_receiver, self.path),
            self.solution_sender,
        )
    }

    /// Construct new work solver from engine receiver and associated channel to send the results
    pub fn new(
        node: node::DynInfo,
        engine_receiver: EngineReceiver,
        solution_queue_tx: mpsc::UnboundedSender<Solution>,
    ) -> Self {
        Self {
            path: vec![node],
            engine_receiver,
            solution_sender: SolutionSender(solution_queue_tx),
        }
    }

    /// Create another solver based on previous one.
    /// It provides generic way how to describe hierarchy in various backends.
    /// Each solver has unique path described by generic node info.
    pub fn branch(&self, node: node::DynInfo) -> Self {
        let mut path = self.path.clone();
        path.push(node);
        Self {
            path,
            engine_receiver: self.engine_receiver.clone(),
            solution_sender: self.solution_sender.clone(),
        }
    }
}

/// Generator is responsible for accepting a `WorkEngine` and draining as much
/// `MiningWork` as possible from it.
pub struct Generator {
    /// Unique path describing internal hierarchy of backend solvers
    path: node::SharedPath,
    /// Source of trait objects that implement `WorkEngine` interface
    engine_receiver: EngineReceiver,
}

impl Generator {
    pub fn new(engine_receiver: EngineReceiver, path: node::Path) -> Self {
        Self {
            path: node::SharedPath::new(path),
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
            work.path.extend(self.path.iter().cloned());
            return Some(work);
        }
    }
}

/// This struct is to be passed to the underlying mining backend. It allows submission of
/// `UniqueMiningWorkSolution`
#[derive(Clone)]
pub struct SolutionSender(mpsc::UnboundedSender<Solution>);

impl SolutionSender {
    pub fn send(&self, solution: Solution) {
        self.0
            .unbounded_send(solution)
            .expect("solution queue send failed");
    }
}
