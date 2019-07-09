use super::*;
use crate::btc;
use crate::hal;
use crate::work;

use crate::misc::LOGGER;
use slog::{info, trace};

use futures::channel::mpsc;
use futures::stream::StreamExt;

use std::sync::{Arc, RwLock};

pub struct Hub;

impl Hub {
    pub fn new() -> (JobSolver, work::Solver) {
        let current_target = Arc::new(RwLock::new(uint::U256::from_big_endian(
            &btc::DIFFICULTY_1_TARGET_BYTES,
        )));
        let (engine_sender, engine_receiver) = engine_channel();
        let (solution_queue_tx, solution_queue_rx) = mpsc::unbounded();
        (
            JobSolver {
                job_sender: hub::JobSender::new(engine_sender, current_target.clone()),
                solution_receiver: hub::JobSolutionReceiver::new(solution_queue_rx, current_target),
            },
            work::Solver::new(Generator::new(engine_receiver), solution_queue_tx),
        )
    }
}

/// Compound object for job submission and solution reception intended to be passed to
/// protocol handler
pub struct JobSolver {
    job_sender: JobSender,
    solution_receiver: JobSolutionReceiver,
}

impl JobSolver {
    pub fn send_job(&mut self, job: Arc<dyn hal::BitcoinJob>) {
        self.job_sender.send(job)
    }

    pub async fn receive_solution(&mut self) -> Option<hal::UniqueMiningWorkSolution> {
        await!(self.solution_receiver.receive())
    }

    pub fn split(self) -> (JobSender, JobSolutionReceiver) {
        (self.job_sender, self.solution_receiver)
    }
}

/// This is the entrypoint for new jobs and updates into processing.
/// Typically the mining protocol handler will inject new jobs through it
pub struct JobSender {
    engine_sender: EngineSender,
    current_target: Arc<RwLock<uint::U256>>,
}

impl JobSender {
    pub fn new(engine_sender: EngineSender, current_target: Arc<RwLock<uint::U256>>) -> Self {
        Self {
            engine_sender,
            current_target,
        }
    }

    pub fn change_target(&self, target: uint::U256) {
        *self
            .current_target
            .write()
            .expect("cannot write to shared current target") = target;
    }

    pub fn send(&mut self, job: Arc<dyn hal::BitcoinJob>) {
        info!(LOGGER, "--- broadcasting new job ---");
        let engine = Arc::new(engine::VersionRolling::new(job, 1));
        self.engine_sender.broadcast(engine);
    }
}

/// Receives `UniqueMiningWorkSolution` via a channel and filters only solutions that meet the
/// pool specified target
pub struct JobSolutionReceiver {
    solution_channel: mpsc::UnboundedReceiver<hal::UniqueMiningWorkSolution>,
    current_target: Arc<RwLock<uint::U256>>,
}

impl JobSolutionReceiver {
    pub fn new(
        solution_channel: mpsc::UnboundedReceiver<hal::UniqueMiningWorkSolution>,
        current_target: Arc<RwLock<uint::U256>>,
    ) -> Self {
        Self {
            solution_channel,
            current_target,
        }
    }

    fn trace_share(solution: &hal::UniqueMiningWorkSolution, target: &uint::U256) {
        // TODO: create specialized structure 'Target' and rewrite it
        let mut xtarget = [0u8; 32];
        target.to_big_endian(&mut xtarget[..]);

        trace!(
            LOGGER,
            "nonce={:08x} bytes={}",
            solution.nonce(),
            hex::encode(&solution.get_block_header().into_bytes()[..])
        );
        trace!(LOGGER, "  hash={:x}", solution.hash());
        trace!(LOGGER, "target={}", hex::encode(xtarget));
    }

    pub async fn receive(&mut self) -> Option<hal::UniqueMiningWorkSolution> {
        while let Some(solution) = await!(self.solution_channel.next()) {
            let current_target = &*self
                .current_target
                .read()
                .expect("cannot read from shared current target");
            if solution.is_valid(current_target) {
                info!(LOGGER, "----- SHARE BELLOW TARGET -----");
                Self::trace_share(&solution, &current_target);
                return Some(solution);
            }
        }
        None
    }
}
