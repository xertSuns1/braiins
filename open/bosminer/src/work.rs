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
mod hub;
mod solver;

use crate::job::{self, Bitcoin};

use ii_bitcoin::{HashTrait, MeetsTarget};

pub use hub::Hub;
pub use solver::{Generator, SolutionSender, Solver};

use futures::channel::mpsc;
use tokio::prelude::*;
use tokio::sync::watch;

use std::cell::Cell;
use std::fmt::{self, Debug};
use std::sync::Arc;
use std::time::SystemTime;

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
/// TODO: add ntime limit for supporting hardware that can do nTime rolling on its own
#[derive(Clone, Debug)]
pub struct Assignment {
    /// Bitcoin job shared with initial network protocol and work solution
    job: Arc<dyn job::Bitcoin>,
    /// Multiple midstates can be generated for each work
    pub midstates: Vec<Midstate>,
    /// Start value for nTime, hardware may roll nTime further
    pub ntime: u32,
}

impl Assignment {
    pub fn new(job: Arc<dyn job::Bitcoin>, midstates: Vec<Midstate>, ntime: u32) -> Self {
        Self {
            job,
            midstates,
            ntime,
        }
    }

    /// Return merkle root tail
    pub fn merkle_root_tail(&self) -> u32 {
        self.job.merkle_root_tail()
    }

    /// Return current target (nBits)
    #[inline]
    pub fn bits(&self) -> u32 {
        self.job.bits()
    }
}

/// Represents raw solution from the mining hardware
#[derive(Clone, Debug)]
pub struct Solution {
    /// actual nonce
    pub nonce: u32,
    /// nTime of the solution in case the HW also rolls the nTime field
    pub ntime: Option<u32>,
    /// index of a midstate that corresponds to the found nonce
    pub midstate_idx: usize,
    /// index of a solution (if multiple were found)
    pub solution_idx: usize,
    /// unique solution identifier
    pub solution_id: u32,
}

/// Container with mining work and a corresponding solution received at a particular time
/// This data structure is used when posting work+solution pairs for further submission upstream.
#[derive(Clone)]
pub struct UniqueSolution {
    /// Time stamp when it has been fetched from the solution FIFO
    timestamp: SystemTime,
    /// Original mining work associated with this solution
    work: Assignment,
    /// Solution of the PoW puzzle
    solution: Solution,
    /// Lazy evaluated double hash of this solution
    hash: Cell<Option<ii_bitcoin::DHash>>,
}

impl UniqueSolution {
    pub fn new(work: Assignment, solution: Solution, timestamp: Option<SystemTime>) -> Self {
        Self {
            timestamp: timestamp.unwrap_or_else(|| SystemTime::now()),
            work,
            solution,
            hash: Cell::new(None),
        }
    }

    pub fn job<T: job::Bitcoin>(&self) -> &T {
        self.work
            .job
            .downcast_ref::<T>()
            .expect("cannot downcast to original job")
    }

    #[inline]
    pub fn nonce(&self) -> u32 {
        self.solution.nonce
    }

    #[inline]
    pub fn time(&self) -> u32 {
        if let Some(time) = self.solution.ntime {
            time
        } else {
            self.work.ntime
        }
    }

    #[inline]
    pub fn version(&self) -> u32 {
        let i = self.solution.midstate_idx;
        self.work.midstates[i].version
    }

    #[inline]
    pub fn midstate_idx(&self) -> usize {
        self.solution.midstate_idx
    }

    /// Return double hash of this solution
    pub fn hash(&self) -> ii_bitcoin::DHash {
        match self.hash.get() {
            Some(value) => value,
            None => {
                // compute hash and store it into cache for later use
                let value = self.get_block_header().hash();
                self.hash.set(Some(value));
                value
            }
        }
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

    pub fn is_valid(&self, current_target: &ii_bitcoin::Target) -> bool {
        if !self.work.job.is_valid() {
            // job is obsolete and has to be flushed
            return false;
        }

        // compute hash for this solution and compare it with target
        self.hash().meets(current_target)
    }
}

impl Debug for UniqueSolution {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:?} (nonce {:08x}, midstate {})",
            self.hash(),
            self.solution.nonce,
            self.solution.midstate_idx
        )
    }
}

pub trait Engine: Debug + Send + Sync {
    fn is_exhausted(&self) -> bool;

    fn next_work(&self) -> LoopState<Assignment>;
}

/// Shared work engine type
pub type DynEngine = Arc<dyn Engine>;

/// Builds a WorkEngine broadcasting channel. The broadcast channel requires an initial value. We
/// use the empty work engine that signals 'exhausted' state all the time.
/// You can optionally pass a channel `reschedule_sender` that will be used to return all exhausted
/// engines. This way you can track what engines are "done".
pub fn engine_channel(
    reschedule_sender: Option<mpsc::UnboundedSender<DynEngine>>,
) -> (EngineSender, EngineReceiver) {
    let work_engine: DynEngine = Arc::new(engine::ExhaustedWork);
    let (sender, receiver) = watch::channel(work_engine);
    (
        EngineSender::new(sender),
        EngineReceiver::new(receiver, reschedule_sender),
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
    reschedule_sender: Option<mpsc::UnboundedSender<DynEngine>>,
}

impl EngineReceiver {
    fn new(
        watch_receiver: watch::Receiver<DynEngine>,
        reschedule_sender: Option<mpsc::UnboundedSender<DynEngine>>,
    ) -> Self {
        Self {
            watch_receiver,
            reschedule_sender,
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
            match await!(self.watch_receiver.next()) {
                // end of stream
                None => return None,
                // new work engine received
                Some(value) => engine = value.expect("cannot receive work engine"),
            }
        }
    }

    /// This function should be called just when last entry has been taken out of engine
    pub fn reschedule(&self) {
        let engine = self.watch_receiver.get_ref().clone();

        // If `reschedule_sender` is present, send the current engine back to it
        if let Some(reschedule_sender) = self.reschedule_sender.as_ref() {
            reschedule_sender
                .unbounded_send(engine)
                .expect("reschedule notify send failed");
        }
    }
}

pub mod test_utils {
    use super::*;
    use crate::test_utils;

    impl From<&test_utils::TestBlock> for Assignment {
        fn from(test_block: &test_utils::TestBlock) -> Self {
            let job = Arc::new(*test_block);
            let time = job.time();

            let mid = Midstate {
                version: job.version(),
                state: job.midstate,
            };

            Self {
                job,
                midstates: vec![mid],
                ntime: time,
            }
        }
    }

    impl From<&test_utils::TestBlock> for Solution {
        fn from(test_block: &test_utils::TestBlock) -> Self {
            Self {
                nonce: test_block.nonce,
                ntime: None,
                midstate_idx: 0,
                solution_idx: 0,
                solution_id: 0,
            }
        }
    }

    impl From<&test_utils::TestBlock> for UniqueSolution {
        fn from(test_block: &test_utils::TestBlock) -> Self {
            Self {
                timestamp: SystemTime::UNIX_EPOCH,
                work: test_block.into(),
                solution: test_block.into(),
                hash: Cell::new(None),
            }
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;

    #[test]
    fn test_block_double_hash() {
        for block in crate::test_utils::TEST_BLOCKS.iter() {
            let solution: UniqueSolution = block.into();

            // test lazy evaluated hash
            let hash = solution.hash();
            assert_eq!(block.hash, hash);

            // test if hash is the same when it is called second time
            let hash = solution.hash();
            assert_eq!(block.hash, hash);
        }
    }
}
