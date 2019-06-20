use crate::hal::{self, BitcoinJob};
use bitcoin_hashes::{sha256d::Hash, Hash as HashTrait};
use byteorder::{ByteOrder, LittleEndian};
use std::sync::Arc;

/// DummyJob to be used for tests
#[derive(Copy, Clone)]
pub struct DummyJob {
    hash: Hash,
    time: u32,
}

impl DummyJob {
    pub fn new() -> Self {
        Self {
            hash: Hash::from_slice(&[0xffu8; 32]).unwrap(),
            time: 0,
        }
    }

    pub fn next(&mut self) {
        self.time += 1;
    }
}

impl hal::BitcoinJob for DummyJob {
    fn version(&self) -> u32 {
        0
    }

    fn version_mask(&self) -> u32 {
        0
    }

    fn previous_hash(&self) -> &Hash {
        &self.hash
    }

    fn merkle_root(&self) -> &Hash {
        &self.hash
    }

    fn time(&self) -> u32 {
        self.time
    }

    fn bits(&self) -> u32 {
        0xffff_ffff
    }
}

/// * `i` - unique identifier for the generated midstate
pub fn prepare_test_work(i: u64) -> hal::MiningWork {
    let job = Arc::new(DummyJob::new());
    let time = job.time();

    let mut mid = hal::Midstate {
        version: 0,
        state: [0u8; 32],
    };
    LittleEndian::write_u64(&mut mid.state, i);

    hal::MiningWork {
        job,
        midstates: vec![mid],
        ntime: time,
    }
}
