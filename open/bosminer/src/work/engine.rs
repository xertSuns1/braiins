use crate::btc;
use crate::hal;

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use bitcoin_hashes::Hash;

// TODO: move to BTC
const VERSION_MASK: u32 = 0x1fffe000;
const VERSION_SHIFT: u32 = 13;

#[derive(Debug)]
pub struct ExhaustedWork;

impl hal::WorkEngine for ExhaustedWork {
    fn is_exhausted(&self) -> bool {
        true
    }

    fn next_work(&self) -> hal::WorkLoop<hal::MiningWork> {
        hal::WorkLoop::Exhausted
    }
}

#[derive(Debug)]
pub struct VersionRolling {
    job: Arc<dyn hal::BitcoinJob>,
    /// Number of midstates that each generated work covers
    midstates: u16,
    /// Starting value of the rolled part of the version (before BIP320 shift)
    curr_version: AtomicU16,
    /// Base Bitcoin block header version with BIP320 bits cleared
    base_version: u32,
}

impl VersionRolling {
    pub fn new(job: Arc<dyn hal::BitcoinJob>, midstates: u16) -> Self {
        let base_version = job.version() & !VERSION_MASK;
        Self {
            job,
            midstates,
            curr_version: AtomicU16::new(0),
            base_version,
        }
    }

    #[inline]
    fn get_block_version(&self, version: u16) -> u32 {
        self.base_version | ((version as u32) << VERSION_SHIFT)
    }

    #[inline]
    fn has_exhausted_range(&self, version: u16) -> bool {
        version.checked_add(self.midstates).is_none()
    }

    /// Roll new versions for the block header for all midstates
    /// Return None If the rolled version space is exhausted. The version range can be
    /// reset by specifying `new_job`
    fn next_versions(&self) -> (Vec<u32>, bool) {
        // Allocate the range for all midstates as per the BIP320 rolled 16 bits
        let curr_version = self.curr_version.load(Ordering::Relaxed);
        let next_version;

        match curr_version.checked_add(self.midstates) {
            Some(value) => {
                next_version = value;
                // TODO: compare_and_swap
                self.curr_version.store(next_version, Ordering::Relaxed);
            }
            None => return (vec![], true),
        }

        // Convert the allocated range to a list of versions as per BIP320
        let mut versions = Vec::with_capacity(self.midstates as usize);
        for version in curr_version..next_version {
            versions.push(self.get_block_version(version));
        }
        (versions, self.has_exhausted_range(next_version))
    }
}

impl hal::WorkEngine for VersionRolling {
    fn is_exhausted(&self) -> bool {
        self.has_exhausted_range(self.curr_version.load(Ordering::Relaxed))
    }

    fn next_work(&self) -> hal::WorkLoop<hal::MiningWork> {
        let (versions, last) = self.next_versions();
        if versions.is_empty() {
            return hal::WorkLoop::Exhausted;
        }

        let mut midstates = Vec::with_capacity(versions.len());
        let mut block_chunk1 = btc::BlockHeader {
            previous_hash: self.job.previous_hash().into_inner(),
            merkle_root: self.job.merkle_root().into_inner(),
            ..Default::default()
        };

        for version in versions {
            block_chunk1.version = version;
            midstates.push(hal::Midstate {
                version,
                state: block_chunk1.midstate(),
            })
        }

        let work = hal::MiningWork {
            job: self.job.clone(),
            midstates,
            ntime: self.job.time(),
        };
        if last {
            hal::WorkLoop::Break(work)
        } else {
            hal::WorkLoop::Continue(work)
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::hal::WorkEngine;
    use crate::test_utils;

    #[test]
    fn test_block_midstate() {
        for block in test_utils::TEST_BLOCKS.iter() {
            let job = Arc::new(*block);
            let engine = VersionRolling::new(job, 1);

            let work = engine.next_work().unwrap();
            assert_eq!(block.midstate, work.midstates[0].state);
        }
    }
}
