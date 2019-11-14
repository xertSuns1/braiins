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

//! Basic components for building WorkEngine broadcasting infrastructure and to send WorkEngines
//! to the actual work solving (mining) backends

pub mod engine;
mod solver;

use crate::hal;
use crate::job;
use crate::node;

use ii_bitcoin::HashTrait as _;

pub use solver::{Generator, SolutionSender, SolverBuilder};

use ii_async_compat::tokio;
use tokio::prelude::*;
use tokio::sync::watch;

use once_cell::sync::OnceCell;

use std::fmt::{self, Debug};
use std::iter;
use std::sync::Arc;
use std::time;

pub enum LoopState<T> {
    /// Mining work is exhausted
    Exhausted,
    /// Returning latest work (subsequent call will return Exhausted)
    Break(T),
    /// Mining work generation will continue
    Continue(T),
}

impl<T> LoopState<T> {
    pub fn unwrap(self) -> T {
        match self {
            LoopState::Break(val) => val,
            LoopState::Continue(val) => val,
            _ => panic!("called `LoopState::unwrap()` on a `None` value"),
        }
    }

    #[inline]
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> LoopState<U> {
        use LoopState::{Break, Continue, Exhausted};

        match self {
            Exhausted => Exhausted,
            Break(x) => Break(f(x)),
            Continue(x) => Continue(f(x)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Midstate {
    /// Version field used for calculating the midstate
    pub version: u32,
    /// Internal state of SHA256 after processing the first chunk (32 bytes)
    pub state: ii_bitcoin::Midstate,
}

/// Describes actual mining work for assignment to a hashing hardware.
/// Starting with merkle_root_tail the data goes to chunk2 of SHA256.
#[derive(Clone, Debug)]
pub struct Assignment {
    /// Unique path describing internal hierarchy of backend solvers
    pub path: node::Path,
    /// Bitcoin job shared with initial network protocol and work solution
    job: Arc<dyn job::Bitcoin>,
    /// Multiple midstates can be generated for each work
    pub midstates: Vec<Midstate>,
    /// nTime value for current work
    pub ntime: u32,
}

impl Assignment {
    pub fn new(job: Arc<dyn job::Bitcoin>, midstates: Vec<Midstate>, ntime: u32) -> Self {
        Self {
            path: vec![],
            job,
            midstates,
            ntime,
        }
    }

    /// Return origin from which the work has been generated
    #[inline]
    pub fn origin(&self) -> Arc<dyn node::Client> {
        self.job.origin()
    }

    /// Return merkle root tail
    #[inline]
    pub fn merkle_root_tail(&self) -> u32 {
        self.job.merkle_root_tail()
    }

    /// Return current target (nBits)
    #[inline]
    pub fn bits(&self) -> u32 {
        self.job.bits()
    }

    /// Return number of generated work associated within this work assignment
    #[inline]
    pub fn generated_work_amount(&self) -> usize {
        self.midstates.len()
    }
}

/// Container with mining work and a corresponding solution received at a particular time
/// This data structure is used when posting work+solution pairs for further submission upstream.
#[derive(Clone)]
pub struct Solution {
    /// Time stamp when it has been fetched from the solution FIFO
    timestamp: time::Instant,
    /// Original mining work associated with this solution
    work: Assignment,
    /// Solution of the PoW puzzle
    solution: Arc<dyn hal::BackendSolution>,
    /// Lazy evaluated double hash of this solution
    hash: OnceCell<ii_bitcoin::DHash>,
}

impl Solution {
    pub fn new(
        work: Assignment,
        solution: impl hal::BackendSolution + 'static,
        timestamp: Option<time::Instant>,
    ) -> Self {
        Self {
            timestamp: timestamp.unwrap_or_else(|| time::Instant::now()),
            work,
            solution: Arc::new(solution),
            hash: OnceCell::new(),
        }
    }

    #[inline]
    pub fn timestamp(&self) -> time::Instant {
        self.timestamp
    }

    pub fn job<T: job::Bitcoin>(&self) -> &T {
        self.work
            .job
            .downcast_ref::<T>()
            .expect("cannot downcast to original job")
    }

    #[inline]
    pub fn nonce(&self) -> u32 {
        self.solution.nonce()
    }

    #[inline]
    pub fn time(&self) -> u32 {
        self.work.ntime
    }

    #[inline]
    pub fn version(&self) -> u32 {
        let i = self.midstate_idx();
        self.work.midstates[i].version
    }

    #[inline]
    pub fn network_target(&self) -> ii_bitcoin::Target {
        // NOTE: it is expected that job has been checked in client and is correct
        ii_bitcoin::Target::from_compact(self.work.job.bits())
            .expect("BUG: job has incorrect nbits")
    }

    #[inline]
    pub fn job_target(&self) -> ii_bitcoin::Target {
        self.work.job.target()
    }

    #[inline]
    pub fn backend_target(&self) -> &ii_bitcoin::Target {
        self.solution.target()
    }

    #[inline]
    pub fn midstate_idx(&self) -> usize {
        self.solution.midstate_idx()
    }

    /// Return double hash of this solution
    pub fn hash(&self) -> &ii_bitcoin::DHash {
        self.hash.get_or_init(|| self.get_block_header().hash())
    }

    /// Converts mining work solution to Bitcoin block header structure which is packable
    pub fn get_block_header(&self) -> ii_bitcoin::BlockHeader {
        let job = &self.work.job;

        ii_bitcoin::BlockHeader {
            version: self.version(),
            previous_hash: job.previous_hash().into_inner(),
            merkle_root: job.merkle_root().into_inner(),
            time: self.time(),
            bits: job.bits(),
            nonce: self.nonce(),
        }
    }

    #[inline]
    pub fn has_valid_job(&self) -> bool {
        self.work.job.is_valid()
    }

    /// Return the whole unique path starting from job origin and ending in backend.
    /// A specified `middleware_path` info can be inserted between these 2 paths. The middleware
    /// info usually represents the miner software itself and is used for overall statistics.
    pub fn path(&self, middleware_path: &node::Path) -> node::Path {
        // Arc does not support dynamic casting to trait bounds so there must be used another Arc
        // indirection with implemented `node::Info` trait.
        // This blanket implementation can be found in the module `crate::node`:
        // impl<T: ?Sized + Info> Info for Arc<T> {}
        let job_origin: node::DynInfo = Arc::new(self.work.job.origin());
        iter::once(&job_origin)
            .chain(middleware_path.iter())
            .chain(self.work.path.iter())
            .cloned()
            .collect()
    }
}

impl Debug for Solution {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:?} (nonce {:08x}, midstate {})",
            self.hash(),
            self.nonce(),
            self.midstate_idx()
        )
    }
}

pub trait Engine: Debug + Send + Sync {
    fn is_exhausted(&self) -> bool;

    fn next_work(&self) -> LoopState<Assignment>;
}

/// Shared work engine type
pub type DynEngine = Arc<dyn Engine>;

/// Interface required by `EngineReceiver` used for notification of exhausted work
pub trait ExhaustedHandler: Send + Sync + 'static {
    /// Called when all work is exhausted in given work engine
    fn handle_exhausted(&self, _engine: DynEngine) {}
}

/// Helper structure for ignoring all events provided by work module
pub struct IgnoreEvents;

impl ExhaustedHandler for IgnoreEvents {}

/// Builds a WorkEngine broadcasting channel. The broadcast channel requires an initial value. We
/// use the empty work engine that signals 'exhausted' state all the time.
/// Only parameter is event handler implementing `ExhaustedHandler` trait that will be used to
/// signal that all work in current engine has been exhausted. This way it is possible to track what
/// engines are "done".
pub fn engine_channel(event_handler: impl ExhaustedHandler) -> (EngineSender, EngineReceiver) {
    let work_engine: DynEngine = Arc::new(engine::ExhaustedWork);
    let (sender, receiver) = watch::channel(work_engine);
    (
        EngineSender::new(sender),
        EngineReceiver::new(receiver, event_handler),
    )
}

/// Sender is responsible for broadcasting a new WorkEngine to all mining
/// backends
pub struct EngineSender {
    inner: watch::Sender<DynEngine>,
}

impl EngineSender {
    fn new(watch_sender: watch::Sender<DynEngine>) -> Self {
        Self {
            inner: watch_sender,
        }
    }

    pub fn broadcast(&mut self, engine: DynEngine) {
        self.inner
            .broadcast(engine)
            .expect("cannot broadcast work engine")
    }
}

/// Manages incoming WorkEngines (see get_engine() for details)
#[derive(Clone)]
pub struct EngineReceiver {
    /// Broadcast channel that is used to distribute current `WorkEngine`
    watch_receiver: watch::Receiver<DynEngine>,
    /// A channel that is (if present) used to send back exhausted engines
    /// to be "recycled" or just so that engine sender is notified that all work
    /// has been generated from them
    event_handler: Arc<dyn ExhaustedHandler>,
}

impl EngineReceiver {
    fn new(
        watch_receiver: watch::Receiver<DynEngine>,
        event_handler: impl ExhaustedHandler,
    ) -> Self {
        Self {
            watch_receiver,
            event_handler: Arc::new(event_handler),
        }
    }

    /// Provides the most recent WorkEngine as long as the engine is able to provide any work.
    /// Otherwise, it sleeps and waits for a new
    pub async fn get_engine(&mut self) -> Option<DynEngine> {
        let mut engine = self.watch_receiver.get_ref().clone();
        loop {
            if !engine.is_exhausted() {
                // return only work engine which can generate some work
                return Some(engine);
            }
            match self.watch_receiver.next().await {
                // end of stream
                None => return None,
                // new work engine received
                Some(value) => engine = value,
            }
        }
    }

    /// This function should be called just when last entry has been taken out of engine
    #[inline]
    pub fn handle_exhausted(&self, engine: DynEngine) {
        self.event_handler.handle_exhausted(engine);
    }
}

#[cfg(test)]
pub mod test {
    use super::*;

    #[test]
    fn test_block_double_hash() {
        for block in crate::test_utils::TEST_BLOCKS.iter() {
            let solution: Solution = block.into();

            // test lazy evaluated hash
            let hash = solution.hash();
            assert_eq!(&block.hash, hash);

            // test if hash is the same when it is called second time
            let hash = solution.hash();
            assert_eq!(&block.hash, hash);
        }
    }
}
