use crate::workhub;
use bitcoin_hashes::{sha256d::Hash, Hash as HashTrait};
use byteorder::ByteOrder;
use downcast_rs::{impl_downcast, Downcast};
use futures_locks::Mutex;
use std::sync::Arc;
use uint;

pub mod s9;

/// Represents interface for Bitcoin job with access to block header from which the new work will be
/// generated. The trait is bound to Downcast which enables connect work solution with original job
/// and hide protocol specific details.
pub trait BitcoinJob: Downcast + Send + Sync {
    /// Original version field that reflects the current network consensus
    fn version(&self) -> u32;
    /// Bit-mask with general purpose bits which can be freely manipulated (specified by BIP320)
    fn version_mask(&self) -> u32;
    /// Double SHA256 hash of the previous block header
    fn previous_hash(&self) -> &Hash;
    /// Double SHA256 hash based on all of the transactions in the block
    fn merkle_root(&self) -> &Hash;
    /// Current block timestamp as seconds since 1970-01-01T00:00 UTC
    fn time(&self) -> u32;
    /// Maximal timestamp for current block as seconds since 1970-01-01T00:00 UTC
    fn max_time(&self) -> u32 {
        self.time()
    }
    /// Current target in compact format (network difficulty)
    /// https://en.bitcoin.it/wiki/Difficulty
    fn bits(&self) -> u32;
}
impl_downcast!(BitcoinJob);

/// Describes actual mining work for submission to a hashing hardware.
/// Starting with merkel_root_lsw the data goes to chunk2 of SHA256.
///
/// NOTE: eventhough, version and extranonce_2 are already included in the midstates, we
/// need them as part of the MiningWork structure. The reason is stratum submission requirements.
/// This may need further refactoring.
/// # TODO
/// Add ntime limit for supporting hardware that can do nTime rolling on its own
#[derive(Clone)]
pub struct MiningWork {
    /// Bitcoin job shared with initial network protocol and work solution
    pub job: Arc<dyn BitcoinJob>,
    /// Version field used for calculating the midstate
    pub version: u32,
    /// Multiple midstates can be generated for each work - these are the full
    pub midstates: Vec<uint::U256>,
    /// Start value for nTime, hardware may roll nTime further.
    pub ntime: u32,
}

impl MiningWork {
    /// Extract least-significant word of merkle root that goes to chunk2 of SHA256
    pub fn merkel_root_lsw<T: ByteOrder>(&self) -> u32 {
        let bytes = &self.job.merkle_root().into_inner();
        T::read_u32(&bytes[bytes.len() - 4..])
    }

    /// Shorthand for getting current target (nBits)
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
    /// time stamp when it has been fetched from the solution FIFO
    pub timestamp: std::time::SystemTime,
    /// Original mining work associated with this solution
    pub work: MiningWork,
    /// solution of the PoW puzzle
    pub solution: MiningWorkSolution,
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

/// Any hardware mining controller should implement at least these methods
pub trait HardwareCtl {
    /// Starts hardware controller connected to workhub, while storing
    /// stats in `a_mining_stats`
    fn start_hw(&self, workhub: workhub::WorkHub, a_mining_stats: Arc<Mutex<MiningStats>>);
}
