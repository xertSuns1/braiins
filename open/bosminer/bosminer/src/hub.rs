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
use crate::job;
use crate::node;
use crate::work;

use futures::channel::mpsc;
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

/// Create Solvers for frontend (pool) and backend (HW accelerator)
pub async fn build_solvers<T: node::WorkSolver + 'static>(
    frontend_info: node::DynInfo,
    backend_work_solver: Arc<T>,
) -> (job::Solver, work::SolverBuilder) {
    let (engine_sender, engine_receiver) = work::engine_channel(EventHandler);
    let (solution_queue_tx, solution_queue_rx) = mpsc::unbounded();
    (
        job::Solver::new(frontend_info, engine_sender, solution_queue_rx),
        work::SolverBuilder::create_root(
            Arc::new(backend::BuildHierarchy),
            backend_work_solver,
            engine_receiver,
            solution_queue_tx,
        )
        .await,
    )
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::test_utils;

    use ii_async_compat::tokio;
    use std::sync::Arc;

    /// This test verifies the whole lifecycle of a mining job, its transformation into work
    /// and also collection of the solution via solution receiver. No actual mining takes place
    /// in the test
    #[tokio::test]
    async fn test_solvers_connection() {
        let (job_solver, work_solver) = build_solvers(
            Arc::new(test_utils::TestInfo::new()),
            Arc::new(test_utils::TestInfo::new()),
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
