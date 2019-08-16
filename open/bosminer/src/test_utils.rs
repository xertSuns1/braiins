use crate::btc::{self, FromHex};
use crate::hal;
use crate::work;

use lazy_static::lazy_static;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard};

/// Real blocks used for tests
#[derive(Copy, Clone)]
pub struct TestBlock {
    pub hash: btc::DHash,
    pub hash_str: &'static str,
    pub midstate: btc::Midstate,
    pub midstate_str: &'static str,
    version: u32,
    prev_hash: btc::DHash,
    merkle_root: btc::DHash,
    time: u32,
    bits: u32,
    pub nonce: u32,
    pub header_bytes: [u8; 80],
    /// The following fields are used for HW specific tests
    /// There are placed here to ensure relation between job and expected result
    /// It mitigate consistency issues when job is removed or new one is added
    pub icarus_bytes: [u8; 64],
}

impl TestBlock {
    pub fn new(
        hash: &'static str,
        midstate: &'static str,
        version: u32,
        prev_hash: &str,
        merkle_root: &str,
        time: u32,
        bits: u32,
        nonce: u32,
        header_bytes: [u8; 80],
        icarus_bytes: [u8; 64],
    ) -> Self {
        Self {
            hash: btc::DHash::from_hex(hash).expect("parse hex"),
            hash_str: hash,
            midstate: btc::Midstate::from_hex(midstate).expect("parse hex"),
            midstate_str: midstate,
            version,
            prev_hash: btc::DHash::from_hex(prev_hash).expect("parse hex"),
            merkle_root: btc::DHash::from_hex(merkle_root).expect("parse hex"),
            time,
            bits,
            nonce,
            header_bytes,
            icarus_bytes,
        }
    }
}

impl std::fmt::Debug for TestBlock {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "{}", self.hash)
    }
}

impl hal::BitcoinJob for TestBlock {
    fn version(&self) -> u32 {
        self.version
    }

    fn version_mask(&self) -> u32 {
        0
    }

    fn previous_hash(&self) -> &btc::DHash {
        &self.prev_hash
    }

    fn merkle_root(&self) -> &btc::DHash {
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
            "e48f544a9a3afa71451471134df6c35682b400254bfe0860c99876bf4679ba4e",
            1,
            "0000000000000488d0b6c4c05f24afe4817a122a1e1a5f009dd391fb0cc1aeb3",
            "ce22a72fa0e9f309830fdb3f75d6c95f051f23ef288a137693ab5c03f2bb6e7e",
            1332160020,
            436941447,
            2726756608,
            [ 0x01, 0x00, 0x00, 0x00, 0xb3, 0xae, 0xc1, 0x0c, 0xfb, 0x91, 0xd3, 0x9d, 0x00, 0x5f,
              0x1a, 0x1e, 0x2a, 0x12, 0x7a, 0x81, 0xe4, 0xaf, 0x24, 0x5f, 0xc0, 0xc4, 0xb6, 0xd0,
              0x88, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7e, 0x6e, 0xbb, 0xf2, 0x03, 0x5c,
              0xab, 0x93, 0x76, 0x13, 0x8a, 0x28, 0xef, 0x23, 0x1f, 0x05, 0x5f, 0xc9, 0xd6, 0x75,
              0x3f, 0xdb, 0x0f, 0x83, 0x09, 0xf3, 0xe9, 0xa0, 0x2f, 0xa7, 0x22, 0xce, 0x14, 0x26,
              0x67, 0x4f, 0x87, 0x32, 0x0b, 0x1a, 0x00, 0x01, 0x87, 0xa2,
            ],
            [ 0x46, 0x79, 0xba, 0x4e, 0xc9, 0x98, 0x76, 0xbf, 0x4b, 0xfe, 0x08, 0x60, 0x82, 0xb4,
              0x00, 0x25, 0x4d, 0xf6, 0xc3, 0x56, 0x45, 0x14, 0x71, 0x13, 0x9a, 0x3a, 0xfa, 0x71,
              0xe4, 0x8f, 0x54, 0x4a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
              0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x87, 0x32, 0x0b, 0x1a,
              0x14, 0x26, 0x67, 0x4f, 0x2f, 0xa7, 0x22, 0xce,
            ],
        ),
        // Sample block from:
        // https://en.bitcoin.it/wiki/Block_hashing_algorithm
        // https://blockchain.info/rawblock/00000000000000001e8d6829a8a21adc5d38d0a473b144b6765798e61f98bd1d
        TestBlock::new(
            "00000000000000001e8d6829a8a21adc5d38d0a473b144b6765798e61f98bd1d",
            "9524c59305c5671316e669ba2d2810a007e86e372f56a9dacd5bce697a78da2d",
            1,
            "00000000000008a3a41b85b8b29ad444def299fee21793cd8b9e567eab02cd81",
            "2b12fcf1b09288fcaff797d71e950e71ae42b91e8bdb2304758dfcffc2b620e3",
            1305998791,
            440711666,
            2504433986,
            [ 0x01, 0x00, 0x00, 0x00, 0x81, 0xcd, 0x02, 0xab, 0x7e, 0x56, 0x9e, 0x8b, 0xcd, 0x93,
              0x17, 0xe2, 0xfe, 0x99, 0xf2, 0xde, 0x44, 0xd4, 0x9a, 0xb2, 0xb8, 0x85, 0x1b, 0xa4,
              0xa3, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xe3, 0x20, 0xb6, 0xc2, 0xff, 0xfc,
              0x8d, 0x75, 0x04, 0x23, 0xdb, 0x8b, 0x1e, 0xb9, 0x42, 0xae, 0x71, 0x0e, 0x95, 0x1e,
              0xd7, 0x97, 0xf7, 0xaf, 0xfc, 0x88, 0x92, 0xb0, 0xf1, 0xfc, 0x12, 0x2b, 0xc7, 0xf5,
              0xd7, 0x4d, 0xf2, 0xb9, 0x44, 0x1a, 0x42, 0xa1, 0x46, 0x95,
            ],
            [ 0x7a, 0x78, 0xda, 0x2d, 0xcd, 0x5b, 0xce, 0x69, 0x2f, 0x56, 0xa9, 0xda, 0x07, 0xe8,
              0x6e, 0x37, 0x2d, 0x28, 0x10, 0xa0, 0x16, 0xe6, 0x69, 0xba, 0x05, 0xc5, 0x67, 0x13,
              0x95, 0x24, 0xc5, 0x93, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
              0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf2, 0xb9, 0x44, 0x1a,
              0xc7, 0xf5, 0xd7, 0x4d, 0xf1, 0xfc, 0x12, 0x2b,
            ],
        ),
        // Sample block v4:
        // https://blockchain.info/rawblock/00000000000000000024974128beb85f6f39d009538f4d92c64d4b82da8a2660
        TestBlock::new(
            "00000000000000000024974128beb85f6f39d009538f4d92c64d4b82da8a2660",
            "9a8378bb5dfc122384cf590facbb1c5af6eca129c32db4a840301c8a60f72b57",
            536870912,
            "000000000000000000262b17185b3c94dff2ab1c4ff6dacb884a80527ec1725d",
            "70ee9e04d1d030770c7c1fda029813067c9327f3b0bde8821666ecf94321ef14",
            1555576766,
            388761373,
            4115486663,
            [ 0x00, 0x00, 0x00, 0x20, 0x5d, 0x72, 0xc1, 0x7e, 0x52, 0x80, 0x4a, 0x88, 0xcb, 0xda,
              0xf6, 0x4f, 0x1c, 0xab, 0xf2, 0xdf, 0x94, 0x3c, 0x5b, 0x18, 0x17, 0x2b, 0x26, 0x00,
              0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x14, 0xef, 0x21, 0x43, 0xf9, 0xec,
              0x66, 0x16, 0x82, 0xe8, 0xbd, 0xb0, 0xf3, 0x27, 0x93, 0x7c, 0x06, 0x13, 0x98, 0x02,
              0xda, 0x1f, 0x7c, 0x0c, 0x77, 0x30, 0xd0, 0xd1, 0x04, 0x9e, 0xee, 0x70, 0xbe, 0x37,
              0xb8, 0x5c, 0x1d, 0x07, 0x2c, 0x17, 0xc7, 0x57, 0x4d, 0xf5,
            ],
            [ 0x60, 0xf7, 0x2b, 0x57, 0x40, 0x30, 0x1c, 0x8a, 0xc3, 0x2d, 0xb4, 0xa8, 0xf6, 0xec,
              0xa1, 0x29, 0xac, 0xbb, 0x1c, 0x5a, 0x84, 0xcf, 0x59, 0x0f, 0x5d, 0xfc, 0x12, 0x23,
              0x9a, 0x83, 0x78, 0xbb, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
              0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1d, 0x07, 0x2c, 0x17,
              0xbe, 0x37, 0xb8, 0x5c, 0x04, 0x9e, 0xee, 0x70,
            ],
        )
    ];
}

/// WorkEngine for testing purposes that carries exactly one piece of `MiningWork`
#[derive(Debug)]
struct OneWorkEngineInner {
    work: Option<hal::MiningWork>,
}

impl OneWorkEngineInner {
    fn is_exhausted(&self) -> bool {
        self.work.is_none()
    }

    fn next_work(&mut self) -> hal::WorkLoop<hal::MiningWork> {
        match self.work.take() {
            Some(work) => hal::WorkLoop::Break(work),
            None => hal::WorkLoop::Exhausted,
        }
    }
}

/// Wrapper for `OneWorkEngineInner` to allow shared access.
#[derive(Debug)]
pub struct OneWorkEngine {
    /// Standard Mutex allows create `TestWorkEngineInner` with mutable self reference in
    /// `next_work` and it also satisfies `hal::WorkEngine` requirement for `Send + Sync`
    inner: StdMutex<OneWorkEngineInner>,
}

impl OneWorkEngine {
    pub fn new(work: hal::MiningWork) -> Self {
        Self {
            inner: StdMutex::new(OneWorkEngineInner { work: Some(work) }),
        }
    }

    fn lock_inner(&self) -> StdMutexGuard<OneWorkEngineInner> {
        self.inner.lock().expect("cannot lock test work engine")
    }
}

impl hal::WorkEngine for OneWorkEngine {
    fn is_exhausted(&self) -> bool {
        self.lock_inner().is_exhausted()
    }

    fn next_work(&self) -> hal::WorkLoop<hal::MiningWork> {
        self.lock_inner().next_work()
    }
}

#[derive(Debug)]
struct TestWorkEngineInner {
    next_test_block: Option<&'static TestBlock>,
    test_block_iter: std::slice::Iter<'static, TestBlock>,
}

impl TestWorkEngineInner {
    fn is_exhausted(&self) -> bool {
        self.next_test_block.is_none()
    }

    fn next_work(&mut self) -> hal::WorkLoop<hal::MiningWork> {
        if self.is_exhausted() {
            return hal::WorkLoop::Exhausted;
        }

        match self.test_block_iter.next() {
            None => hal::WorkLoop::Break(self.next_test_block.take()),
            Some(block) => hal::WorkLoop::Continue(self.next_test_block.replace(block)),
        }
        .map(|block| block.expect("test block is 'None'").into())
    }
}

#[derive(Debug)]
pub struct TestWorkEngine {
    /// Standard Mutex allows create `TestWorkEngineInner` with mutable self reference in
    /// `next_work` and it also satisfies `hal::WorkEngine` requirement for `Send + Sync`
    inner: StdMutex<TestWorkEngineInner>,
}

impl TestWorkEngine {
    pub fn new() -> Self {
        let mut test_block_iter = TEST_BLOCKS.iter();
        let next_test_block = test_block_iter.next();

        Self {
            inner: StdMutex::new(TestWorkEngineInner {
                next_test_block,
                test_block_iter,
            }),
        }
    }

    fn lock_inner(&self) -> StdMutexGuard<TestWorkEngineInner> {
        self.inner.lock().expect("cannot lock test work engine")
    }
}

impl hal::WorkEngine for TestWorkEngine {
    fn is_exhausted(&self) -> bool {
        self.lock_inner().is_exhausted()
    }

    fn next_work(&self) -> hal::WorkLoop<hal::MiningWork> {
        self.lock_inner().next_work()
    }
}

pub fn create_test_work_receiver() -> work::EngineReceiver {
    let work_engine = Arc::new(TestWorkEngine::new());
    let (mut sender, receiver) = work::engine_channel();
    sender.broadcast(work_engine);
    receiver
}

pub fn create_test_work_generator() -> work::Generator {
    work::Generator::new(create_test_work_receiver())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::hal::BitcoinJob;
    use crate::utils::compat_block_on;

    fn get_engine(work_receiver: &mut work::EngineReceiver) -> Arc<hal::WorkEngine> {
        compat_block_on(work_receiver.get_engine()).expect("cannot get test work engine")
    }

    fn cmp_block_with_work(block: &TestBlock, work: hal::MiningWork) -> hal::MiningWork {
        assert_eq!(block.midstate, work.midstates[0].state);
        assert_eq!(block.merkle_root_tail(), work.merkle_root_tail());
        assert_eq!(block.time(), work.ntime);
        assert_eq!(block.bits(), work.bits());
        work
    }

    #[test]
    fn test_work_receiver() {
        let mut work_receiver = create_test_work_receiver();
        let test_engine = get_engine(&mut work_receiver);

        // test work engine is not exhausted so it should return the same engine
        assert!(Arc::ptr_eq(&test_engine, &get_engine(&mut work_receiver)));

        let mut work_break = false;
        for block in TEST_BLOCKS.iter() {
            match test_engine
                .next_work()
                .map(|work| cmp_block_with_work(block, work))
            {
                hal::WorkLoop::Exhausted => {
                    panic!("test work generator returned less work than expected")
                }
                hal::WorkLoop::Break(_) => {
                    assert!(!work_break, "test work generator returned double break");
                    work_break = true;
                }
                hal::WorkLoop::Continue(_) => {
                    assert!(!work_break, "test work generator continues after break")
                }
            }
        }
        assert!(
            work_break,
            "test work generator returned more work than expected"
        );
        match test_engine.next_work() {
            hal::WorkLoop::Exhausted => (),
            _ => panic!("test work generator continues after returning all work"),
        };
    }
}
