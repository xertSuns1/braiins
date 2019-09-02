use ii_bitcoin::HashTrait;

use crate::hal::{self, BitcoinJob};

use std::sync::Arc;

use byteorder::{ByteOrder, LittleEndian};

/// NullJob to be used for chip initialization and tests
#[derive(Debug, Copy, Clone)]
pub struct NullJob {
    hash: ii_bitcoin::DHash,
    time: u32,
    bits: u32,
    version: u32,
}

impl NullJob {
    /// XXX: maybe create a structure with named members to pass to this constructor, otherwise it's confusing.
    pub fn new(time: u32, bits: u32, version: u32) -> Self {
        Self {
            hash: ii_bitcoin::DHash::from_slice(&[0xffu8; 32]).unwrap(),
            time,
            bits,
            version,
        }
    }

    pub fn next(&mut self) {
        self.time += 1;
    }
}

impl hal::BitcoinJob for NullJob {
    fn version(&self) -> u32 {
        self.version
    }

    fn version_mask(&self) -> u32 {
        0
    }

    fn previous_hash(&self) -> &ii_bitcoin::DHash {
        &self.hash
    }

    fn merkle_root(&self) -> &ii_bitcoin::DHash {
        &self.hash
    }

    fn time(&self) -> u32 {
        self.time
    }

    fn bits(&self) -> u32 {
        self.bits
    }

    fn is_valid(&self) -> bool {
        true
    }
}

/// * `i` - unique identifier for the generated midstate
pub fn prepare(i: u64) -> hal::MiningWork {
    let job = Arc::new(NullJob::new(0, 0xffff_ffff, 0));
    let time = job.time();

    let mut midstate_bytes = [0u8; ii_bitcoin::SHA256_DIGEST_SIZE];
    LittleEndian::write_u64(&mut midstate_bytes, i);

    let mid = hal::Midstate {
        version: 0,
        state: midstate_bytes.into(),
    };

    hal::MiningWork::new(job, vec![mid], time)
}

pub fn prepare_opencore(enable_core: bool, midstate_count: usize) -> hal::MiningWork {
    let bits = if enable_core { 0xffff_ffff } else { 0 };
    let job = Arc::new(NullJob::new(0, bits, 0));
    let time = job.time();

    let one_midstate = hal::Midstate {
        version: 0,
        state: [0u8; ii_bitcoin::SHA256_DIGEST_SIZE].into(),
    };

    hal::MiningWork::new(job, vec![one_midstate; midstate_count], time)
}
