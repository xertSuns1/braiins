use super::*;
use crate::hal::{self, WorkEngine};
use crate::work::engine;

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
    job_queue: JobQueue,
    engine: Option<engine::VersionRolling>,
}

impl Generator {
    pub fn new(job_channel: JobChannelReceiver) -> Self {
        Self {
            job_queue: JobQueue::new(job_channel),
            engine: None,
        }
    }

    /// Returns new work generated from the current job
    pub async fn generate(&mut self) -> Option<hal::MiningWork> {
        loop {
            let (job, new_job) = await!(self.job_queue.get_job());
            if new_job {
                self.engine = Some(engine::VersionRolling::new(job, 1));
            }
            let work = self
                .engine
                .as_mut()
                .expect("missing work engine")
                .next_work();
            if work.is_none() {
                self.job_queue.finish_current_job();
            } else {
                return work;
            }
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
