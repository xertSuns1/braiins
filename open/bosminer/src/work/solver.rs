use super::*;
use crate::hal;

use crate::misc::LOGGER;
use slog::warn;

use futures::channel::mpsc;

/// Compound object that is supposed to be sent down to the mining backend that can in turn solve
/// any generated work and submit solutions.
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
        solution_queue_tx: mpsc::UnboundedSender<hal::UniqueMiningWorkSolution>,
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
    engine_receiver: EngineReceiver,
}

impl Generator {
    pub fn new(engine_receiver: EngineReceiver) -> Self {
        Self { engine_receiver }
    }

    /// Loops until new work is available or no more `WorkEngines` are supplied (signals
    /// Generator shutdown)
    pub async fn generate(&mut self) -> Option<hal::MiningWork> {
        loop {
            let work;
            match await!(self.engine_receiver.get_engine()) {
                // end of stream
                None => return None,
                // try to generate new work from engine
                Some(engine) => work = engine.next_work(),
            }
            return Some(match work {
                // one or more competing work engines are exhausted
                // try to gen new work engine
                // NOTE: this can happen simultaneously for multiple parallel generators because
                // only one can win the last work and so there should not be included any logging
                hal::WorkLoop::Exhausted => continue,
                // consecutive call of work engine may return new work
                hal::WorkLoop::Continue(value) => value,
                // tha last work is returned from work engine (the work is exhausted)
                hal::WorkLoop::Break(value) => {
                    warn!(LOGGER, "No more work available for current job!");
                    // inform the 'WorkHub' for rescheduling a new work
                    self.engine_receiver.reschedule();
                    value
                }
            });
        }
    }
}

/// This struct is to be passed to the underlying mining backend. It allows submission of
/// `UniqueMiningWorkSolution`
#[derive(Clone)]
pub struct SolutionSender(mpsc::UnboundedSender<hal::UniqueMiningWorkSolution>);

impl SolutionSender {
    pub fn send(&self, solution: hal::UniqueMiningWorkSolution) {
        self.0
            .unbounded_send(solution)
            .expect("solution queue send failed");
    }
}
