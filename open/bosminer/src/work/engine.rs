//! Provides work engines that are capable for converting Jobs to actual work suitable for mining
//! backend processing
use crate::hal;

use ii_bitcoin::HashTrait;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

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

/// BIP320 specifies sixteen bits in block header nVersion field
/// The maximal index represent the range which is excluded so it must be incremented by 1.
const BIP320_MAX_INDEX: u32 = ii_bitcoin::BIP320_VERSION_MAX + 1;

/// Primitive for atomic range counter
/// This structure can be freely shared among parallel processes and each range is returned only to
/// one competing process. The structure returns ranges until maximal allowed index is reached.
#[derive(Debug, Clone)]
struct AtomicRange {
    /// Maximal index value which cannot be exceeded
    max_index: u32,
    /// Size of step between each range
    step_size: u32,
    /// Current index used as a starting index for next range
    curr_index: Arc<AtomicU32>,
}

impl AtomicRange {
    /// Construct atomic range iterator with following parameters:
    /// `step_size` - size of step for each range
    /// `start_index` - starting index returned in the first range
    /// <start_index, start_index + step_size)
    /// `max_index` - maximal index returned in the last range
    /// <max_index - step_size, max_index)
    pub fn new(start_index: u32, max_index: u32, step_size: u32) -> Self {
        assert!(start_index <= max_index);
        assert!(step_size > 0);
        Self {
            max_index,
            step_size,
            curr_index: Arc::new(AtomicU32::new(start_index)),
        }
    }

    /// Try to add some `count` to `current` value with check that the result does not exceed
    /// maximal index
    fn checked_add(&self, current: u32, count: u32) -> Option<u32> {
        current
            .checked_add(count)
            .filter(|value| *value <= self.max_index)
    }

    /// Atomically get current index which will be used for next returned range
    fn get_current(&self) -> u32 {
        self.curr_index.load(Ordering::Relaxed)
    }

    /// Concurrently determine next range of indexes
    /// Return `None` if the available space is exhausted.
    /// The starting index is included and ending one is excluded: <a, b).
    pub fn next(&self) -> Option<(u32, u32)> {
        loop {
            let current = self.get_current();
            return match self.checked_add(current, self.step_size) {
                None => None,
                Some(next) => {
                    if self
                        .curr_index
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

    /// Check if given version cannot be used for next range
    pub fn is_exhausted<T: Into<Option<u32>>>(&self, current: T) -> bool {
        let current = current.into().unwrap_or_else(|| self.get_current());
        self.checked_add(current, self.step_size).is_none()
    }
}

/// Version rolling implements WorkEngine trait and represents a shared source of work for mining
/// backends. Each instance takes care of atomically allocating version field ranges until the
/// range is full exhausted
#[derive(Debug, Clone)]
pub struct VersionRolling {
    job: Arc<dyn hal::BitcoinJob>,
    /// Number of midstates that each generated work covers
    midstate_count: usize,
    /// Current range of the rolled part of the version (before BIP320 shift)
    curr_range: AtomicRange,
    /// Base Bitcoin block header version with BIP320 bits cleared
    base_version: u32,
}

impl VersionRolling {
    pub fn new(job: Arc<dyn hal::BitcoinJob>, midstate_count: usize) -> Self {
        let base_version = job.version() & !ii_bitcoin::BIP320_VERSION_MASK;
        Self {
            job,
            midstate_count,
            curr_range: AtomicRange::new(0, BIP320_MAX_INDEX, midstate_count as u32),
            base_version,
        }
    }

    /// Convert the allocated index to a block version as per BIP320
    #[inline]
    fn get_block_version(&self, index: u32) -> u32 {
        assert!(index <= ii_bitcoin::BIP320_VERSION_MAX);
        self.base_version | (index << ii_bitcoin::BIP320_VERSION_SHIFT)
    }
}

impl hal::WorkEngine for VersionRolling {
    fn is_exhausted(&self) -> bool {
        self.curr_range.is_exhausted(None)
    }

    fn next_work(&self) -> hal::WorkLoop<hal::MiningWork> {
        // determine next range of indexes from version space
        let (current, next) = match self.curr_range.next() {
            // return immediately when the space is exhausted
            None => return hal::WorkLoop::Exhausted,
            // use range of indexes for generation of midstates
            Some(range) => range,
        };

        // check if given range is the same as number of midstates
        assert_eq!(self.midstate_count, (next - current) as usize);
        let mut midstates = Vec::with_capacity(self.midstate_count);

        // prepare block chunk1 with all invariants
        let mut block_chunk1 = ii_bitcoin::BlockHeader {
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
        if self.curr_range.is_exhausted(next) {
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
    use crate::hal::BitcoinJob;
    use crate::hal::WorkEngine;
    use crate::test_utils;

    fn compare_range(start: u32, stop: u32, step: u32) {
        let range = AtomicRange::new(start, stop, step);
        for i in (start..stop - (step - 1)).step_by(step as usize) {
            assert_eq!(range.next(), Some((i, i + step)));
        }
        assert_eq!(range.next(), None);
    }

    #[test]
    fn test_atomic_range() {
        compare_range(0, 1, 1);
        compare_range(
            ii_bitcoin::BIP320_VERSION_MAX - 1,
            ii_bitcoin::BIP320_VERSION_MAX,
            1,
        );
        compare_range(std::u32::MAX - 1, std::u32::MAX, 1);

        compare_range(0, 2, 1);
        compare_range(1, 2, 1);

        compare_range(0, 2, 2);
        compare_range(0, 3, 2);
        compare_range(0, 4, 2);
        compare_range(1, 4, 2);
        compare_range(2, 4, 2);

        compare_range(0, 4, 4);
        compare_range(0, 5, 4);
        compare_range(0, 6, 4);
        compare_range(0, 7, 4);
        compare_range(0, 8, 4);
        compare_range(0, 9, 4);
        compare_range(1, 9, 4);
        compare_range(2, 9, 4);
        compare_range(3, 9, 4);
        compare_range(4, 9, 4);
        compare_range(5, 9, 4);
    }

    #[test]
    fn test_block_midstate() {
        for block in test_utils::TEST_BLOCKS.iter() {
            let job = Arc::new(*block);
            let engine = VersionRolling::new(job, 1);

            let work = engine.next_work().unwrap();
            assert_eq!(block.midstate, work.midstates[0].state);
        }
    }

    fn get_block_version(job: &Arc<test_utils::TestBlock>, index: u32) -> u32 {
        job.version() | (index << ii_bitcoin::BIP320_VERSION_SHIFT)
    }

    #[test]
    fn test_exhausted_work() {
        // use first test block for job
        let job = Arc::new(test_utils::TEST_BLOCKS[0]);
        let engine = VersionRolling::new(job.clone(), 1);

        // modify current version counter to decrease the search space
        // adn test only boundary values
        const START_INDEX: u32 = ii_bitcoin::BIP320_VERSION_MAX - 1;
        engine
            .curr_range
            .curr_index
            .store(START_INDEX, Ordering::Relaxed);
        assert!(!engine.is_exhausted());

        match engine.next_work() {
            hal::WorkLoop::Continue(work) => assert_eq!(
                get_block_version(&job, START_INDEX),
                work.midstates[0].version
            ),
            _ => panic!("expected 'hal::WorkLoop::Continue'"),
        }
        assert!(!engine.is_exhausted());

        match engine.next_work() {
            hal::WorkLoop::Break(work) => assert_eq!(
                get_block_version(&job, ii_bitcoin::BIP320_VERSION_MAX),
                work.midstates[0].version
            ),
            _ => panic!("expected 'hal::WorkLoop::Break'"),
        }
        assert!(engine.is_exhausted());

        match engine.next_work() {
            hal::WorkLoop::Exhausted => {}
            _ => panic!("expected 'hal::WorkLoop::Exhausted'"),
        }
        assert!(engine.is_exhausted());
    }
}
