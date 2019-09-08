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

use ii_logging::macros::*;

use super::*;
use crate::job;
use crate::runtime_config;
use crate::stats;
use crate::work;

use futures::channel::mpsc;
use futures::stream::StreamExt;

use std::sync::{Arc, RwLock};

/// Top level builder for `JobSolver` and `work::Solver` intended to be used when instantiating
/// the full miner
pub struct Hub;

impl Hub {
    /// Create Solvers for frontend (pool) and backend (HW accelerator)
    pub fn build_solvers() -> (JobSolver, work::Solver) {
        let (engine_sender, engine_receiver) = engine_channel(None);
        let (solution_queue_tx, solution_queue_rx) = mpsc::unbounded();
        (
            JobSolver::new(engine_sender, solution_queue_rx),
            work::Solver::new(engine_receiver, solution_queue_tx),
        )
    }
}

/// Helper function for creating target difficulty suitable for sharing
pub fn create_shared_target(target: ii_bitcoin::Target) -> Arc<RwLock<ii_bitcoin::Target>> {
    Arc::new(RwLock::new(target))
}

/// Compound object for job submission and solution reception intended to be passed to
/// protocol handler
pub struct JobSolver {
    job_sender: JobSender,
    solution_receiver: JobSolutionReceiver,
}

impl JobSolver {
    pub fn new(
        engine_sender: EngineSender,
        solution_queue_rx: mpsc::UnboundedReceiver<work::UniqueSolution>,
    ) -> Self {
        let current_target = create_shared_target(Default::default());
        Self {
            job_sender: JobSender::new(engine_sender, current_target.clone()),
            solution_receiver: JobSolutionReceiver::new(solution_queue_rx, current_target),
        }
    }

    pub fn split(self) -> (JobSender, JobSolutionReceiver) {
        (self.job_sender, self.solution_receiver)
    }
}

/// This is the entrypoint for new jobs and updates into processing.
/// Typically the mining protocol handler will inject new jobs through it
pub struct JobSender {
    engine_sender: EngineSender,
    current_target: Arc<RwLock<ii_bitcoin::Target>>,
}

impl JobSender {
    pub fn new(
        engine_sender: EngineSender,
        current_target: Arc<RwLock<ii_bitcoin::Target>>,
    ) -> Self {
        Self {
            engine_sender,
            current_target,
        }
    }

    pub fn change_target(&self, target: ii_bitcoin::Target) {
        *self
            .current_target
            .write()
            .expect("cannot write to shared current target") = target;
    }

    pub fn send(&mut self, job: Arc<dyn job::Bitcoin>) {
        info!("--- broadcasting new job ---");
        let engine = Arc::new(engine::VersionRolling::new(
            job,
            runtime_config::get_midstate_count(),
        ));
        self.engine_sender.broadcast(engine);
    }
}

/// Receives `UniqueMiningWorkSolution` via a channel and filters only solutions that meet the
/// pool specified target
pub struct JobSolutionReceiver {
    solution_channel: mpsc::UnboundedReceiver<work::UniqueSolution>,
    current_target: Arc<RwLock<ii_bitcoin::Target>>,
}

impl JobSolutionReceiver {
    pub fn new(
        solution_channel: mpsc::UnboundedReceiver<work::UniqueSolution>,
        current_target: Arc<RwLock<ii_bitcoin::Target>>,
    ) -> Self {
        Self {
            solution_channel,
            current_target,
        }
    }

    fn trace_share(solution: &work::UniqueSolution, target: &ii_bitcoin::Target) {
        info!(
            "nonce={:08x} bytes={}",
            solution.nonce(),
            hex::encode(&solution.get_block_header().into_bytes()[..])
        );
        info!("  hash={:x}", solution.hash());
        info!("target={:x}", target);
    }

    pub async fn receive(&mut self) -> Option<work::UniqueSolution> {
        while let Some(solution) = await!(self.solution_channel.next()) {
            let current_target = &*self
                .current_target
                .read()
                .expect("cannot read from shared current target");
            if solution.is_valid(current_target) {
                stats::account_solution(&current_target);
                info!("----- Found share within current job's difficulty (diff={}) target range -----",
                    current_target.get_difficulty());
                Self::trace_share(&solution, &current_target);
                return Some(solution);
            }
        }
        None
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::test_utils;

    /// This test verifies the whole lifecycle of a mining job, its transformation into work
    /// and also collection of the solution via solution receiver. No actual mining takes place
    /// in the test
    #[test]
    fn test_solvers_connection() {
        let (job_solver, work_solver) = work::Hub::build_solvers();

        let (mut job_sender, mut solution_receiver) = job_solver.split();
        let (mut work_generator, solution_sender) = work_solver.split();

        // default target is be set to difficulty 1 so all solution should pass
        for block in test_utils::TEST_BLOCKS.iter() {
            let job = Arc::new(*block);

            // send prepared testing block to job solver
            job_sender.send(job);
            // work generator receives this job and prepares work from it
            let work = ii_async_compat::block_on(work_generator.generate()).unwrap();
            // initial value for version rolling is 0 so midstate should match with expected one
            assert_eq!(block.midstate, work.midstates[0].state);
            // test block has automatic conversion into work solution
            solution_sender.send(block.into());
            // this solution should pass through job solver
            let solution = ii_async_compat::block_on(solution_receiver.receive()).unwrap();
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
        assert!(ii_async_compat::block_on(work_generator.generate()).is_some());
    }

    /// Helper function that compares 2 hashes while interpreting them as targets
    fn double_hash_cmp(a: &ii_bitcoin::DHash, b: &ii_bitcoin::DHash) -> std::cmp::Ordering {
        let a_target: ii_bitcoin::Target = (*a).into();
        let b_target: ii_bitcoin::Target = (*b).into();
        a_target.cmp(&b_target)
    }

    /// This test verifies that after changing mining target, the job solver filters the provided
    /// solutions and yields only those that meet the target.
    #[test]
    fn test_job_solver_target() {
        let (engine_sender, _) = engine_channel(None);
        let (solution_queue_tx, solution_queue_rx) = mpsc::unbounded();

        let (job_sender, mut solution_receiver) =
            JobSolver::new(engine_sender, solution_queue_rx).split();

        // default target is be set to difficulty 1 so all solution should pass
        for block in test_utils::TEST_BLOCKS.iter() {
            // test block has automatic conversion into work solution
            solution_queue_tx.unbounded_send(block.into()).unwrap();
            // this solution should pass through job solver
            let solution = ii_async_compat::block_on(solution_receiver.receive()).unwrap();
            // check if the solution is equal to expected one
            assert_eq!(block.nonce, solution.nonce());
        }

        // find test block with lowest hash which will be set as a target
        let target_block = test_utils::TEST_BLOCKS
            .iter()
            .min_by(|a, b| double_hash_cmp(&a.hash, &b.hash))
            .unwrap();

        // change the target to return from solution receiver only this block
        let target: ii_bitcoin::Target = target_block.hash.into();
        job_sender.change_target(target);

        // send all solutions to the queue not to block on receiver
        for block in test_utils::TEST_BLOCKS.iter() {
            // test block has automatic conversion into work solution
            solution_queue_tx.unbounded_send(block.into()).unwrap();
        }

        // send target block again to get two results and ensures that all blocks has been processed
        solution_queue_tx
            .unbounded_send(target_block.into())
            .unwrap();

        // check if the solutions is equal to expected ones
        let solution = ii_async_compat::block_on(solution_receiver.receive()).unwrap();
        assert_eq!(target_block.nonce, solution.nonce());
        let solution = ii_async_compat::block_on(solution_receiver.receive()).unwrap();
        assert_eq!(target_block.nonce, solution.nonce());
    }
}
