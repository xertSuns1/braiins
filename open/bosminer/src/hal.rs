use crate::workhub;

use crate::misc::LOGGER;
use slog::{info, trace};

use futures::channel::mpsc;
use futures::stream::StreamExt;
use futures_locks::Mutex;

use std::sync::Arc;
use std::mem;

use bitcoin_hashes::{sha256d, sha256d::Hash, Hash as HashTrait};
use byteorder::{ByteOrder, LittleEndian};
use downcast_rs::{impl_downcast, Downcast};

pub mod s9;

/// A Bitcoin block header is 80 bytes long
const BITCOIN_BLOCK_HEADER_SIZE: usize = 80;

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
    /// Checks if job is still valid for mining
    fn is_valid(&self) -> bool;
}
impl_downcast!(BitcoinJob);

#[derive(Clone, Debug)]
pub struct Midstate {
    /// Version field used for calculating the midstate
    pub version: u32,
    /// Internal state of SHA256 after processing the first chunk (32 bytes)
    pub state: [u8; 32],
}

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
    /// Multiple midstates can be generated for each work
    pub midstates: Vec<Midstate>,
    /// Start value for nTime, hardware may roll nTime further.
    pub ntime: u32,
}

impl MiningWork {
    /// Extract least-significant word of merkle root that goes to chunk2 of SHA256
    pub fn merkel_root_lsw<T: ByteOrder>(&self) -> u32 {
        let bytes = &self.job.merkle_root().into_inner();
        T::read_u32(&bytes[bytes.len() - mem::size_of::<u32>()..])
    }

    /// Shortcut for getting current target (nBits)
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
    work: MiningWork,
    /// solution of the PoW puzzle
    solution: MiningWorkSolution,
}

impl UniqueMiningWorkSolution {
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
    pub fn time_offset(&self) -> u16 {
        let job_time = self.work.job.time();
        let offset = self
            .time()
            .checked_sub(job_time)
            .expect("job time offset overflow");
        assert!(offset <= u16::max_value().into());
        offset as u16
    }

    #[inline]
    pub fn version(&self) -> u32 {
        let i = self.solution.midstate_idx;
        self.work.midstates[i].version
    }

    pub fn get_block_bytes(&self) -> [u8; BITCOIN_BLOCK_HEADER_SIZE] {
        let job = &self.work.job;
        let buffer = &mut [0u8; 80];

        LittleEndian::write_u32(&mut buffer[0..4], self.version());
        buffer[4..36].copy_from_slice(&job.previous_hash().into_inner());
        buffer[36..68].copy_from_slice(&job.merkle_root().into_inner());
        LittleEndian::write_u32(&mut buffer[68..72], self.time());
        LittleEndian::write_u32(&mut buffer[72..76], job.bits());
        LittleEndian::write_u32(&mut buffer[76..80], self.nonce());

        *buffer
    }

    pub fn compute_sha256d(&self) -> Hash {
        let block_bytes = self.get_block_bytes();
        // compute SHA256 double hash
        sha256d::Hash::hash(&block_bytes[..])
    }

    pub fn trace_share(&self, hash: uint::U256, target: uint::U256) {
        let mut xtarget = [0u8; 32];
        target.to_big_endian(&mut xtarget[..]);
        let mut xhash = [0u8; 32];
        hash.to_big_endian(&mut xhash[..]);

        trace!(
            LOGGER,
            "nonce={:08x} bytes={}",
            self.nonce(),
            hex::encode(&self.get_block_bytes()[..])
        );
        trace!(LOGGER, "  hash={}", hex::encode(xhash));
        trace!(LOGGER, "target={}", hex::encode(xtarget));
    }

    pub fn is_valid(&self, current_target: &uint::U256) -> bool {
        if !self.work.job.is_valid() {
            // job is obsolete and has to be flushed
            return false;
        }

        // compute hash for this solution
        let double_hash = self.compute_sha256d();
        // convert it to number suitable for target comparison
        let double_hash_u256 = uint::U256::from_little_endian(&double_hash.into_inner());
        // and check it with current target (pool difficulty)
        let ok = double_hash_u256 <= *current_target;
        if ok {
            self.trace_share(double_hash_u256, *current_target)
        }
        ok
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

/// Any hardware mining controller should implement at least these methods
pub trait HardwareCtl {
    /// Starts hardware controller connected to workhub, while storing
    /// stats in `a_mining_stats`
    fn start_hw(
        &self,
        workhub: workhub::WorkHub,
        a_mining_stats: Arc<Mutex<MiningStats>>,
        shutdown: ShutdownSender,
    );
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
                timestamp: std::time::SystemTime::UNIX_EPOCH,
                work: test_block.into(),
                solution: test_block.into(),
            }
        }
    }

    #[test]
    fn test_block_double_hash() {
        for block in test_utils::TEST_BLOCKS.iter() {
            let solution: UniqueMiningWorkSolution = block.into();

            let hash = solution.compute_sha256d();
            assert_eq!(block.hash, hash);
        }
    }
}
