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

#[cfg(feature = "erupter")]
pub mod erupter;
#[cfg(feature = "antminer_s9")]
pub mod s9;

/// Reexport HAL entry point for selected target to unify interface
#[cfg(feature = "erupter")]
pub use erupter::{
    config,
    error::{Error, ErrorKind},
    run,
};
#[cfg(feature = "antminer_s9")]
pub use s9::{
    config,
    error::{Error, ErrorKind},
    run,
};

use ii_bitcoin::{HashTrait, MeetsTarget};

use std::cell::Cell;
use std::convert::TryInto;
use std::fmt::{self, Debug};
use std::mem;
use std::sync::Arc;
use std::time::SystemTime;

use downcast_rs::{impl_downcast, Downcast};

/// Represents interface for Bitcoin job with access to block header from which the new work will be
/// generated. The trait is bound to Downcast which enables connect work solution with original job
/// and hide protocol specific details.
pub trait BitcoinJob: Debug + Downcast + Send + Sync {
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
impl_downcast!(BitcoinJob);

pub enum WorkLoop<T> {
    /// Mining work is exhausted
    Exhausted,
    /// Returning latest work (subsequent call will return Exhausted)
    Break(T),
    /// Mining work generation will continue
    Continue(T),
}

impl<T> WorkLoop<T> {
    pub fn unwrap(self) -> T {
        match self {
            WorkLoop::Break(val) => val,
            WorkLoop::Continue(val) => val,
            _ => panic!("called `WorkLoop::unwrap()` on a `None` value"),
        }
    }

    #[inline]
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> WorkLoop<U> {
        use WorkLoop::{Break, Continue, Exhausted};

        match self {
            Exhausted => Exhausted,
            Break(x) => Break(f(x)),
            Continue(x) => Continue(f(x)),
        }
    }
}

pub trait WorkEngine: Debug + Send + Sync {
    fn is_exhausted(&self) -> bool;

    fn next_work(&self) -> WorkLoop<MiningWork>;
}

#[derive(Clone, Debug)]
pub struct Midstate {
    /// Version field used for calculating the midstate
    pub version: u32,
    /// Internal state of SHA256 after processing the first chunk (32 bytes)
    pub state: ii_bitcoin::Midstate,
}

/// Describes actual mining work for submission to a hashing hardware.
/// Starting with merkle_root_tail the data goes to chunk2 of SHA256.
/// TODO: add ntime limit for supporting hardware that can do nTime rolling on its own
#[derive(Clone, Debug)]
pub struct MiningWork {
    /// Bitcoin job shared with initial network protocol and work solution
    job: Arc<dyn BitcoinJob>,
    /// Multiple midstates can be generated for each work
    pub midstates: Vec<Midstate>,
    /// Start value for nTime, hardware may roll nTime further
    pub ntime: u32,
}

impl MiningWork {
    pub fn new(job: Arc<dyn BitcoinJob>, midstates: Vec<Midstate>, ntime: u32) -> Self {
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
pub struct MiningWorkSolution {
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
pub struct UniqueMiningWorkSolution {
    /// Time stamp when it has been fetched from the solution FIFO
    timestamp: SystemTime,
    /// Original mining work associated with this solution
    work: MiningWork,
    /// Solution of the PoW puzzle
    solution: MiningWorkSolution,
    /// Lazy evaluated double hash of this solution
    hash: Cell<Option<ii_bitcoin::DHash>>,
}

impl UniqueMiningWorkSolution {
    pub fn new(
        work: MiningWork,
        solution: MiningWorkSolution,
        timestamp: Option<SystemTime>,
    ) -> Self {
        Self {
            timestamp: timestamp.unwrap_or_else(|| SystemTime::now()),
            work,
            solution,
            hash: Cell::new(None),
        }
    }

    pub fn job<T: BitcoinJob>(&self) -> &T {
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

impl Debug for UniqueMiningWorkSolution {
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

pub mod test_utils {
    use super::*;
    use crate::test_utils;

    impl From<&test_utils::TestBlock> for MiningWork {
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

    impl From<&test_utils::TestBlock> for MiningWorkSolution {
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

    impl From<&test_utils::TestBlock> for UniqueMiningWorkSolution {
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
            let solution: UniqueMiningWorkSolution = block.into();

            // test lazy evaluated hash
            let hash = solution.hash();
            assert_eq!(block.hash, hash);

            // test if hash is the same when it is called second time
            let hash = solution.hash();
            assert_eq!(block.hash, hash);
        }
    }
}
