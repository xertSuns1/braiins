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
use crate::stats::{self, DiffTargetType};
use crate::work;

use futures::channel::mpsc;
use futures::stream::StreamExt;
use ii_async_compat::futures;

use std::convert::TryInto;
use std::fmt::Debug;
use std::mem;
use std::sync::{Arc, Weak};

use downcast_rs::{impl_downcast, Downcast};

/// Represents interface for Bitcoin job with access to block header from which the new work will be
/// generated. The trait is bound to Downcast which enables connect work solution with original job
/// and hide protocol specific details.
pub trait Bitcoin: Debug + Downcast + Send + Sync {
    /// Information about origin where the job has been created
    fn origin(&self) -> Weak<dyn node::Client>;
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
    pub job_sender: Sender,
    pub solution_receiver: SolutionReceiver,
}

impl Solver {
    pub fn new(
        engine_sender: Arc<work::EngineSender>,
        solution_receiver: mpsc::UnboundedReceiver<work::Solution>,
    ) -> Self {
        Self {
            job_sender: Sender::new(engine_sender),
            solution_receiver: SolutionReceiver::new(solution_receiver),
        }
    }
}

/// This is the entrypoint for new jobs and updates into processing.
/// Typically the mining protocol handler will inject new jobs through it
pub struct Sender {
    engine_sender: Arc<work::EngineSender>,
}

impl Sender {
    pub fn new(engine_sender: Arc<work::EngineSender>) -> Self {
        Self { engine_sender }
    }

    /// Check if the job has valid attributes
    fn job_sanity_check(
        job: &Arc<dyn job::Bitcoin>,
        origin: &Option<Arc<dyn node::Client>>,
    ) -> bool {
        let mut valid = true;
        if let Err(msg) = ii_bitcoin::Target::from_compact(job.bits()) {
            error!(
                "Invalid job's nBits ({}) received from '{}'",
                msg,
                origin
                    .as_ref()
                    .map(|client| client.to_string())
                    .unwrap_or("?".to_string())
            );
            valid = false;
        }
        valid
    }

    pub fn send(&self, job: Arc<dyn job::Bitcoin>) {
        let origin = job.origin().upgrade();
        if !Self::job_sanity_check(&job, &origin) {
            origin.map(|origin| origin.client_stats().invalid_jobs().inc());
            return;
        }

        // send only jobs with correct data
        if let Some(origin) = origin {
            origin.client_stats().valid_jobs().inc();
            info!("--- broadcasting new job ---");
            self.engine_sender.broadcast_job(job);
        } else {
            // Origin has been removed and no one will receive any solution
            info!("--- discarding job ---");
        }
    }

    #[inline]
    pub fn invalidate(&self) {
        self.engine_sender.invalidate();
    }
}

/// Receives `work::Solution` via a channel and filters only solutions that meet the client/pool
/// specified target
#[derive(Debug)]
pub struct SolutionReceiver {
    solution_channel: mpsc::UnboundedReceiver<work::Solution>,
}

impl SolutionReceiver {
    pub fn new(solution_channel: mpsc::UnboundedReceiver<work::Solution>) -> Self {
        Self { solution_channel }
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
            let path = solution.path();
            let time = solution.timestamp();
            let hash = solution.hash();
            let job_target = solution.job_target();

            // compare block hash for given solution with all targets
            // TODO: create tests for solution validation with all difficulty variants
            assert!(&solution.network_target() <= job_target);
            if hash.meets(&solution.network_target()) {
                stats::account_valid_solution(&path, &solution, time, DiffTargetType::Network)
                    .await;
            } else if hash.meets(&job_target) {
                stats::account_valid_solution(&path, &solution, time, DiffTargetType::Job).await;
            } else if hash.meets(solution.backend_target()) {
                stats::account_valid_solution(&path, &solution, time, DiffTargetType::Backend)
                    .await;
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

    /// Empty all buffered solutions without blocking. This is to prevent the client from submitting
    /// already stale solutions
    /// TODO: We should review this regularly as there may be extensions in the mining protocol that
    /// may allow resume a mining session
    pub fn flush(&mut self) {
        while let Ok(Some(_)) = self.solution_channel.try_next() {}
    }
}
