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

use ii_bitcoin::HashTrait;

use crate::job;
use crate::runtime_config;
use crate::stats;
use crate::work;

use futures::channel::mpsc;
use futures::stream::StreamExt;

use std::convert::TryInto;
use std::fmt::Debug;
use std::mem;
use std::sync::{Arc, RwLock};

use downcast_rs::{impl_downcast, Downcast};

/// Represents interface for Bitcoin job with access to block header from which the new work will be
/// generated. The trait is bound to Downcast which enables connect work solution with original job
/// and hide protocol specific details.
pub trait Bitcoin: Debug + Downcast + Send + Sync {
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
    /// Current target in compact format (network difficulty)
    /// https://en.bitcoin.it/wiki/Difficulty
    fn bits(&self) -> u32;
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

/// Helper function for creating target difficulty suitable for sharing
pub fn create_shared_target(target: ii_bitcoin::Target) -> Arc<RwLock<ii_bitcoin::Target>> {
    Arc::new(RwLock::new(target))
}

/// Compound object for job submission and solution reception intended to be passed to
/// protocol handler
pub struct Solver {
    job_sender: Sender,
    solution_receiver: SolutionReceiver,
}

impl Solver {
    pub fn new(
        engine_sender: work::EngineSender,
        solution_queue_rx: mpsc::UnboundedReceiver<work::Solution>,
    ) -> Self {
        let current_target = create_shared_target(Default::default());
        Self {
            job_sender: Sender::new(engine_sender, current_target.clone()),
            solution_receiver: SolutionReceiver::new(solution_queue_rx, current_target),
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
    current_target: Arc<RwLock<ii_bitcoin::Target>>,
}

impl Sender {
    pub fn new(
        engine_sender: work::EngineSender,
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
        let engine = Arc::new(work::engine::VersionRolling::new(
            job,
            runtime_config::get_midstate_count(),
        ));
        self.engine_sender.broadcast(engine);
    }
}

/// Receives `work::UniqueSolution` via a channel and filters only solutions that meet the
/// pool specified target
pub struct SolutionReceiver {
    solution_channel: mpsc::UnboundedReceiver<work::Solution>,
    current_target: Arc<RwLock<ii_bitcoin::Target>>,
}

impl SolutionReceiver {
    pub fn new(
        solution_channel: mpsc::UnboundedReceiver<work::Solution>,
        current_target: Arc<RwLock<ii_bitcoin::Target>>,
    ) -> Self {
        Self {
            solution_channel,
            current_target,
        }
    }

    fn trace_share(solution: &work::Solution, target: &ii_bitcoin::Target) {
        info!(
            "nonce={:08x} bytes={}",
            solution.nonce(),
            hex::encode(&solution.get_block_header().into_bytes()[..])
        );
        info!("  hash={:x}", solution.hash());
        info!("target={:x}", target);
    }

    pub async fn receive(&mut self) -> Option<work::Solution> {
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
