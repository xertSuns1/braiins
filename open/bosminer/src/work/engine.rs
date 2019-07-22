//! Provides work engines that are capable for converting Jobs to actual work suitable for mining
//! backend processing
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

/// Version rolling implements WorkEngine trait and represents a shared source of work for mining
/// backends. Each instance takes care of atomically allocating version field ranges until the
/// range is full exhausted
#[derive(Debug, Clone)]
pub struct VersionRolling {
    job: Arc<dyn hal::BitcoinJob>,
    /// Number of midstates that each generated work covers
    midstates: u16,
    /// Starting value of the rolled part of the version (before BIP320 shift)
    curr_version: Arc<AtomicU16>,
    /// Base Bitcoin block header version with BIP320 bits cleared
    base_version: u32,
}

impl VersionRolling {
    pub fn new(job: Arc<dyn hal::BitcoinJob>, midstates: u16) -> Self {
        let base_version = job.version() & !VERSION_MASK;
        Self {
            job,
            midstates,
            curr_version: Arc::new(AtomicU16::new(0)),
            base_version,
        }
    }

    /// Convert the allocated index to a block version as per BIP320
    #[inline]
    fn get_block_version(&self, index: u16) -> u32 {
        self.base_version | ((index as u32) << VERSION_SHIFT)
    }

    /// Check if given version cannot be used for next range
    #[inline]
    fn has_exhausted_range(&self, version: u16) -> bool {
        version.checked_add(self.midstates).is_none()
    }

    /// Concurrently determine next range of indexes for rolling version
    /// Return None If the rolled version space is exhausted.
    fn next_range(&self) -> Option<(u16, u16)> {
        loop {
            let current = self.curr_version.load(Ordering::Relaxed);
            return match current.checked_add(self.midstates) {
                None => None,
                Some(next) => {
                    if self
                        .curr_version
                        .compare_and_swap(current, next, Ordering::Relaxed)
                        != current
                    {
                        // try it again when concurrent task has been faster
                        continue;
                    }
                    Some((current, next))
                }
            };
        }
    }
}

impl hal::WorkEngine for VersionRolling {
    fn is_exhausted(&self) -> bool {
        self.has_exhausted_range(self.curr_version.load(Ordering::Relaxed))
    }

    fn next_work(&self) -> hal::WorkLoop<hal::MiningWork> {
        // determine next range of indexes from version space
        let (current, next) = match self.next_range() {
            // return immediately when the space is exhausted
            None => return hal::WorkLoop::Exhausted,
            // use range of indexes for generation of midstates
            Some(range) => range,
        };

        // check if given range is the same as number of midstates
        assert_eq!(self.midstates, next - current);
        let mut midstates = Vec::with_capacity(self.midstates as usize);

        // prepare block chunk1 with all invariants
        let mut block_chunk1 = btc::BlockHeader {
            previous_hash: self.job.previous_hash().into_inner(),
            merkle_root: self.job.merkle_root().into_inner(),
            ..Default::default()
        };

        // generate all midstates from given range of indexes
        for index in current..next {
            // use index for generation compatible header version
            let version = self.get_block_version(index);
            block_chunk1.version = version;
            midstates.push(hal::Midstate {
                version,
                state: block_chunk1.midstate(),
            })
        }

        let work = hal::MiningWork::new(self.job.clone(), midstates, self.job.time());
        if self.has_exhausted_range(next) {
            // when the whole version space has been exhausted then mark the generated work as
            // a last one (the next call of this method will return 'Exhausted')
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
