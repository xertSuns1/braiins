use super::*;
use crate::hal;

use futures::channel::mpsc;

/// Workhub sources jobs from `job_queue` and uses `work_generator` to convert them to
/// actual `MiningWork` suitable for processing (solving) by actual mining backend
pub struct Solver {
    /// Work generator for converting jobs to MiningWork
    work_generator: Generator,
    solution_sender: SolutionSender,
}

impl Solver {
    /// Hardware-facing API
    pub async fn generate_work(&mut self) -> Option<hal::MiningWork> {
        await!(self.work_generator.generate())
    }

    /// Hardware-facing API
    pub fn send_solution(&self, solution: hal::UniqueMiningWorkSolution) {
        self.solution_sender.send(solution);
    }

    pub fn split(self) -> (Generator, SolutionSender) {
        (self.work_generator, self.solution_sender)
    }

    /// Construct new WorkHub and associated queue to send work through
    /// This is runner/orchestrator/pump-facing function
    pub fn new(
        work_generator: Generator,
        solution_queue_tx: mpsc::UnboundedSender<hal::UniqueMiningWorkSolution>,
    ) -> Self {
        Self {
            work_generator,
            solution_sender: SolutionSender(solution_queue_tx),
        }
    }
}

/// Generates `MiningWork` by rolling the version field of the block header
pub struct Generator {
    engine_receiver: EngineReceiver,
}

impl Generator {
    pub fn new(engine_receiver: EngineReceiver) -> Self {
        Self { engine_receiver }
    }

    /// Returns new work generated from the current job
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
                hal::WorkLoop::Exhausted => continue,
                // consecutive call of work engine may return new work
                hal::WorkLoop::Continue(value) => value,
                // tha last work is returned from work engine (the work is exhausted)
                hal::WorkLoop::Break(value) => {
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
