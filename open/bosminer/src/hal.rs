#[cfg(feature = "erupter")]
pub mod erupter;
#[cfg(feature = "antminer_s9")]
pub mod s9;

/// Reexport HAL entry point for selected target to unify interface
#[cfg(feature = "erupter")]
pub use erupter::{
    error::{Error, ErrorKind},
    run,
};
#[cfg(feature = "antminer_s9")]
pub use s9::{
    error::{Error, ErrorKind},
    run,
};

use crate::btc::{self, HashTrait, MeetsTarget};

use futures::channel::mpsc;
use futures::stream::StreamExt;

use std::cell::Cell;
use std::convert::TryInto;
use std::fmt::Debug;
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
    fn previous_hash(&self) -> &btc::Hash;
    /// Double SHA256 hash based on all of the transactions in the block
    fn merkle_root(&self) -> &btc::Hash;
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
    /// Returning latest work (sequential call will return Exhausted)
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
    pub state: btc::Midstate,
}

/// Describes actual mining work for submission to a hashing hardware.
/// Starting with merkle_root_tail the data goes to chunk2 of SHA256.
/// TODO: add ntime limit for supporting hardware that can do nTime rolling on its own
#[derive(Clone)]
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
    /// Unique identifier for the solution
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
    hash: Cell<Option<btc::Hash>>,
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

    /// Return double hash of this solution
    pub fn hash(&self) -> btc::Hash {
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
    pub fn get_block_header(&self) -> btc::BlockHeader {
        let job = &self.work.job;

        btc::BlockHeader {
            version: self.version(),
            previous_hash: job.previous_hash().into_inner(),
            merkle_root: job.merkle_root().into_inner(),
            time: self.time(),
            bits: job.bits(),
            nonce: self.nonce(),
        }
    }

    pub fn is_valid(&self, current_target: &btc::Target) -> bool {
        if !self.work.job.is_valid() {
            // job is obsolete and has to be flushed
            return false;
        }

        // compute hash for this solution and compare it with target
        self.hash().meets(current_target)
    }
}

/// Holds all hardware-related statistics for a hashchain
pub struct MiningStats {
    /// Number of work items generated for the hardware
    pub work_generated: usize,
    /// Number of stale solutions received from the hardware
    pub stale_solutions: u64,
    /// Unable to feed the hardware fast enough results in duplicate solutions as
    /// multiple chips may process the same mining work
    pub duplicate_solutions: u64,
    /// Keep track of nonces that didn't match with previously received solutions (after
    /// filtering hardware errors, this should really stay at 0, otherwise we have some weird
    /// hardware problem)
    pub mismatched_solution_nonces: u64,
    /// Counter of unique solutions
    pub unique_solutions: u64,
}

impl MiningStats {
    pub fn new() -> Self {
        Self {
            work_generated: 0,
            stale_solutions: 0,
            duplicate_solutions: 0,
            mismatched_solution_nonces: 0,
            unique_solutions: 0,
        }
    }
}

/// Message used for shutdown synchronization
pub type ShutdownMsg = &'static str;

/// Sender side of shutdown messanger
#[derive(Clone)]
pub struct ShutdownSender(mpsc::UnboundedSender<ShutdownMsg>);

impl ShutdownSender {
    pub fn send(&self, msg: ShutdownMsg) {
        self.0.unbounded_send(msg).expect("send failed");
    }
}

/// Receiver side of shutdown messanger
pub struct ShutdownReceiver(mpsc::UnboundedReceiver<ShutdownMsg>);

impl ShutdownReceiver {
    pub async fn receive(&mut self) -> ShutdownMsg {
        let reply = await!(self.0.next());

        // TODO: do we have to handle all these cases?
        let msg = match reply {
            None => "all hchains died",
            Some(m) => m,
        };
        msg
    }
}

/// Shutdown messanger constructor & splitter
pub struct Shutdown(ShutdownSender, ShutdownReceiver);

impl Shutdown {
    pub fn new() -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::unbounded();
        Self(ShutdownSender(shutdown_tx), ShutdownReceiver(shutdown_rx))
    }
    pub fn split(self) -> (ShutdownSender, ShutdownReceiver) {
        (self.0, self.1)
    }
}

#[cfg(test)]
pub mod test {
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

    #[test]
    fn test_block_double_hash() {
        for block in test_utils::TEST_BLOCKS.iter() {
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
