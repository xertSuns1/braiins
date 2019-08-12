use crate::btc::{self, HashTrait};
use crate::hal::{self, BitcoinJob};

use std::sync::Arc;

use byteorder::{ByteOrder, LittleEndian};

/// NullJob to be used for chip initialization and tests
#[derive(Debug, Copy, Clone)]
pub struct NullJob {
    hash: btc::DHash,
    time: u32,
}

impl NullJob {
    pub fn new(time: u32) -> Self {
        Self {
            hash: btc::DHash::from_slice(&[0xffu8; 32]).unwrap(),
            time,
        }
    }

    pub fn next(&mut self) {
        self.time += 1;
    }
}

impl hal::BitcoinJob for NullJob {
    fn version(&self) -> u32 {
        0
    }

    fn version_mask(&self) -> u32 {
        0
    }

    fn previous_hash(&self) -> &btc::DHash {
        &self.hash
    }

    fn merkle_root(&self) -> &btc::DHash {
        &self.hash
    }

    fn time(&self) -> u32 {
        self.time
    }

    fn bits(&self) -> u32 {
        0xffff_ffff
    }

    fn is_valid(&self) -> bool {
        true
    }
}

/// * `i` - unique identifier for the generated midstate
pub fn prepare(i: u64) -> hal::MiningWork {
    let job = Arc::new(NullJob::new(0));
    let time = job.time();

    let mut midstate_bytes = [0u8; btc::SHA256_DIGEST_SIZE];
    LittleEndian::write_u64(&mut midstate_bytes, i);

    let mid = hal::Midstate {
        version: 0,
        state: midstate_bytes.into(),
    };

    hal::MiningWork::new(job, vec![mid], time)
}
