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

use futures::channel::mpsc;
use ii_async_compat::futures;

/// Compound object that is supposed to be sent down to the mining backend that can in turn solve
/// any generated work and submit solutions.
#[derive(Clone)]
pub struct Solver {
    /// Work generator for sourcing `MiningWork`
    work_generator: Generator,
    /// Solution submission channel for the underlying mining backend
    solution_sender: SolutionSender,
}

impl Solver {
    pub fn split(self) -> (Generator, SolutionSender) {
        (self.work_generator, self.solution_sender)
    }

    /// Construct new work solver from engine receiver and associated channel to send the results
    pub fn new(
        engine_receiver: EngineReceiver,
        solution_queue_tx: mpsc::UnboundedSender<Solution>,
    ) -> Self {
        Self {
            work_generator: Generator::new(engine_receiver),
            solution_sender: SolutionSender(solution_queue_tx),
        }
    }
}

/// Generator is responsible for accepting a `WorkEngine` and draining as much
/// `MiningWork` as possible from it.
#[derive(Clone)]
pub struct Generator {
    /// Source of trait objects that implement `WorkEngine` interface
    engine_receiver: EngineReceiver,
}

impl Generator {
    pub fn new(engine_receiver: EngineReceiver) -> Self {
        Self { engine_receiver }
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
            return Some(match engine.next_work() {
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
            });
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
