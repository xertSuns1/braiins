use crate::hal::{self, BitcoinJob};

use lazy_static::lazy_static;
use std::sync::Arc;

use bitcoin_hashes::{hex::FromHex, sha256d::Hash, Hash as HashTrait};
use byteorder::{ByteOrder, LittleEndian};

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

    fn is_valid(&self) -> bool {
        true
    }
}

/// Real blocks used for tests
#[derive(Copy, Clone)]
pub struct TestBlock {
    pub hash: Hash,
    version: u32,
    prev_hash: Hash,
    merkle_root: Hash,
    time: u32,
    bits: u32,
    pub nonce: u32,
}

impl TestBlock {
    pub fn new(
        hash: &str,
        version: u32,
        prev_hash: &str,
        merkle_root: &str,
        time: u32,
        bits: u32,
        nonce: u32,
    ) -> Self {
        Self {
            hash: Hash::from_hex(hash).expect("parse hex"),
            version,
            prev_hash: Hash::from_hex(prev_hash).expect("parse hex"),
            merkle_root: Hash::from_hex(merkle_root).expect("parse hex"),
            time,
            bits,
            nonce,
        }
    }
}

impl hal::BitcoinJob for TestBlock {
    fn version(&self) -> u32 {
        self.version
    }

    fn version_mask(&self) -> u32 {
        0
    }

    fn previous_hash(&self) -> &Hash {
        &self.prev_hash
    }

    fn merkle_root(&self) -> &Hash {
        &self.merkle_root
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

lazy_static! {
    pub static ref TEST_BLOCKS: Vec<TestBlock> = vec![
        // Block 171874 binary representation
        // https://blockchain.info/rawblock/00000000000004b64108a8e4168cfaa890d62b8c061c6b74305b7f6cb2cf9fda
        TestBlock::new(
            "00000000000004b64108a8e4168cfaa890d62b8c061c6b74305b7f6cb2cf9fda",
            1,
            "0000000000000488d0b6c4c05f24afe4817a122a1e1a5f009dd391fb0cc1aeb3",
            "ce22a72fa0e9f309830fdb3f75d6c95f051f23ef288a137693ab5c03f2bb6e7e",
            1332160020,
            436941447,
            2726756608,
        ),
        // Sample block from:
        // https://en.bitcoin.it/wiki/Block_hashing_algorithm
        // https://blockchain.info/rawblock/00000000000000001e8d6829a8a21adc5d38d0a473b144b6765798e61f98bd1d
        TestBlock::new(
            "00000000000000001e8d6829a8a21adc5d38d0a473b144b6765798e61f98bd1d",
            1,
            "00000000000008a3a41b85b8b29ad444def299fee21793cd8b9e567eab02cd81",
            "2b12fcf1b09288fcaff797d71e950e71ae42b91e8bdb2304758dfcffc2b620e3",
            1305998791,
            440711666,
            2504433986,
        ),
        // Sample block v4:
        // https://blockchain.info/rawblock/00000000000000000024974128beb85f6f39d009538f4d92c64d4b82da8a2660
        TestBlock::new(
            "00000000000000000024974128beb85f6f39d009538f4d92c64d4b82da8a2660",
            536870912,
            "000000000000000000262b17185b3c94dff2ab1c4ff6dacb884a80527ec1725d",
            "70ee9e04d1d030770c7c1fda029813067c9327f3b0bde8821666ecf94321ef14",
            1555576766,
            388761373,
            4115486663,
        )
    ];
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
