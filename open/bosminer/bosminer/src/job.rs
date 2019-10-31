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

use ii_bitcoin::{HashTrait as _, MeetsTarget};

use crate::job;
use crate::node;
use crate::runtime_config;
use crate::stats::{self, DiffTargetType};
use crate::work;

use futures::channel::mpsc;
use futures::stream::StreamExt;
use ii_async_compat::futures;

use std::convert::TryInto;
use std::fmt::Debug;
use std::mem;
use std::sync::Arc;

use downcast_rs::{impl_downcast, Downcast};

/// Represents interface for Bitcoin job with access to block header from which the new work will be
/// generated. The trait is bound to Downcast which enables connect work solution with original job
/// and hide protocol specific details.
pub trait Bitcoin: Debug + Downcast + Send + Sync {
    /// Information about origin where the job has been created
    fn origin(&self) -> node::DynInfo;
    /// Original version field that reflects the current network consensus
    fn version(&self) -> u32;
    /// Bit-mask with general purpose bits which can be freely manipulated (specified by BIP320)
    fn version_mask(&self) -> u32;
    /// Double SHA256 hash of the previous block header
    fn previous_hash(&self) -> &ii_bitcoin::DHash;
    /// Double SHA256 hash based on all of the transactions in the block
    fn merkle_root(&self) -> &ii_bitcoin::DHash;
    /// Current block timestamp as seconds since 1970-01-01T00:00 UTC
    fn time(&self) -> u32;
    /// Maximal timestamp for current block as seconds since 1970-01-01T00:00 UTC
    fn max_time(&self) -> u32 {
        self.time()
    }
    /// Current network target in compact format (network difficulty)
    /// https://en.bitcoin.it/wiki/Difficulty
    fn bits(&self) -> u32;
    /// Current pool/protocol target used for solution checking
    fn target(&self) -> ii_bitcoin::Target;
    /// Checks if job is still valid for mining
    fn is_valid(&self) -> bool;

    /// Extract least-significant word of merkle root that goes to chunk2 of SHA256
    /// The word is interpreted as a little endian number.
    #[inline]
    fn merkle_root_tail(&self) -> u32 {
        let merkle_root = self.merkle_root().into_inner();
        u32::from_le_bytes(
            merkle_root[merkle_root.len() - mem::size_of::<u32>()..]
                .try_into()
                .expect("slice with incorrect length"),
        )
    }
}
impl_downcast!(Bitcoin);

/// Compound object for job submission and solution reception intended to be passed to
/// protocol handler
pub struct Solver {
    job_sender: Sender,
    solution_receiver: SolutionReceiver,
}

impl Solver {
    pub fn new(
        frontend_info: node::DynInfo,
        engine_sender: work::EngineSender,
        solution_queue_rx: mpsc::UnboundedReceiver<work::Solution>,
    ) -> Self {
        Self {
            job_sender: Sender::new(engine_sender),
            solution_receiver: SolutionReceiver::new(frontend_info, solution_queue_rx),
        }
    }

    pub fn split(self) -> (Sender, SolutionReceiver) {
        (self.job_sender, self.solution_receiver)
    }
}

/// This is the entrypoint for new jobs and updates into processing.
/// Typically the mining protocol handler will inject new jobs through it
pub struct Sender {
    engine_sender: work::EngineSender,
}

impl Sender {
    pub fn new(engine_sender: work::EngineSender) -> Self {
        Self { engine_sender }
    }

    pub fn send(&mut self, job: Arc<dyn job::Bitcoin>) {
        info!("--- broadcasting new job ---");
        let engine = Arc::new(work::engine::VersionRolling::new(
            job,
            runtime_config::get_midstate_count(),
        ));
        self.engine_sender.broadcast(engine);
    }
}

/// Receives `work::Solution` via a channel and filters only solutions that meet the client/pool
/// specified target
pub struct SolutionReceiver {
    frontend_path: node::SharedPath,
    solution_channel: mpsc::UnboundedReceiver<work::Solution>,
}

impl SolutionReceiver {
    pub fn new(
        frontend_info: node::DynInfo,
        solution_channel: mpsc::UnboundedReceiver<work::Solution>,
    ) -> Self {
        Self {
            frontend_path: node::SharedPath::new(vec![frontend_info]),
            solution_channel,
        }
    }

    fn trace_share(solution: &work::Solution, target: &ii_bitcoin::Target) {
        info!(
            "----- Found share within current job's difficulty (diff={}) target range -----",
            target.get_difficulty()
        );
        info!(
            "nonce={:08x} bytes={}",
            solution.nonce(),
            hex::encode(&solution.get_block_header().into_bytes()[..])
        );
        info!("  hash={:x}", solution.hash());
        info!("target={:x}", target);
    }

    pub async fn receive(&mut self) -> Option<work::Solution> {
        while let Some(solution) = self.solution_channel.next().await {
            let path = solution.path(&self.frontend_path);
            let time = solution.timestamp();
            let hash = solution.hash();
            let job_target = solution.job_target();

            // compare block hash for given solution with all targets
            // TODO: create tests for solution validation with all difficulty variants
            assert!(solution.network_target() <= job_target);
            if hash.meets(&solution.network_target()) {
                stats::account_valid_diff(&path, &solution, time, DiffTargetType::NETWORK).await;
            } else if hash.meets(&job_target) {
                stats::account_valid_diff(&path, &solution, time, DiffTargetType::JOB).await;
            } else if hash.meets(solution.backend_target()) {
                stats::account_valid_diff(&path, &solution, time, DiffTargetType::BACKEND).await;
                // skip submitting the solution as we've only met backend difficulty
                continue;
            } else {
                stats::account_error_backend_diff(&path, &solution.backend_target(), time).await;
                // skip submitting the solution as this is a backend error
                continue;
            }

            if solution.has_valid_job() {
                // TODO: Account solution to Discard meter
                Self::trace_share(&solution, &job_target);
                return Some(solution);
            }
        }
        None
    }
}
