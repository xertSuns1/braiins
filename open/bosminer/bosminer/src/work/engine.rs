// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

//! Provides work engines that are capable for converting Jobs to actual work suitable for mining
//! backend processing
use super::*;
use crate::job;

use ii_bitcoin::HashTrait;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub struct ExhaustedWork;

impl Engine for ExhaustedWork {
    fn is_exhausted(&self) -> bool {
        true
    }

    fn next_work(&self) -> LoopState<Assignment> {
        LoopState::Exhausted
    }
}

/// BIP320 specifies sixteen bits in block header nVersion field
/// The maximal index represent the range which is excluded so it must be incremented by 1.
const BIP320_UPPER_BOUND_EXCLUSIVE_INDEX: u32 = ii_bitcoin::BIP320_VERSION_MAX + 1;
/// Once we exhaust the version we roll, we have to roll ntime.
/// The current limit gives us support for miners with speed up to 2.4 PH/s
/// hash_space * roll_ntime_seconds / new_stratum_job_every_sec = 2**(32 + 16) * 256 / 30 = 2.4e15
const ROLL_NTIME_SECONDS: u32 = 256;

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
/// range is full exhausted. After version has been rolled over, ntime is incremented and version
/// resetted to 0. The limit of `ntime` range is determined by `ROLL_NTIME_SECONDS`.
///
/// TODO: Rolling ntime together with version IS A HACK. This needs to be fixed properly by raising
/// `ntime` in sync with real-time clock.
#[derive(Debug, Clone)]
pub struct VersionRolling {
    job: Arc<dyn job::Bitcoin>,
    /// Number of midstates that each generated work covers
    midstate_count: usize,
    /// Current range of the rolled part of the version (before BIP320 shift)
    /// We keep current version in lower 16 bits and `ntime_offset`
    /// in upper 8 bits. When version overflows, the ntime_offset gets
    /// automatically incremented.
    curr_range: AtomicRange,
    /// Base Bitcoin block header version with BIP320 bits cleared
    base_version: u32,
}

impl VersionRolling {
    pub fn new(job: Arc<dyn job::Bitcoin>, midstate_count: usize) -> Self {
        let base_version = job.version() & !ii_bitcoin::BIP320_VERSION_MASK;
        // we have to be sure we have no "leftover" midstates when we roll
        assert_eq!(
            BIP320_UPPER_BOUND_EXCLUSIVE_INDEX % (midstate_count as u32),
            0
        );
        Self {
            job,
            midstate_count,
            curr_range: AtomicRange::new(
                0,
                BIP320_UPPER_BOUND_EXCLUSIVE_INDEX * ROLL_NTIME_SECONDS,
                midstate_count as u32,
            ),
            base_version,
        }
    }

    /// Convert the allocated index to a block version as per BIP320
    #[inline]
    fn get_block_version(&self, index: u32) -> u32 {
        let version = index % BIP320_UPPER_BOUND_EXCLUSIVE_INDEX;
        assert!(version <= ii_bitcoin::BIP320_VERSION_MAX);
        self.base_version | (version << ii_bitcoin::BIP320_VERSION_SHIFT)
    }

    /// Convert the allocated index to a ntime offset
    #[inline]
    fn get_ntime_offset(&self, index: u32) -> u32 {
        let ntime_offset = index / BIP320_UPPER_BOUND_EXCLUSIVE_INDEX;
        assert!(ntime_offset < ROLL_NTIME_SECONDS);
        ntime_offset
    }
}

impl Engine for VersionRolling {
    fn is_exhausted(&self) -> bool {
        self.curr_range.is_exhausted(None)
    }

    fn next_work(&self) -> LoopState<Assignment> {
        // determine next range of indexes from version space
        let (current, next) = match self.curr_range.next() {
            // return immediately when the space is exhausted
            None => return LoopState::Exhausted,
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
            midstates.push(Midstate {
                version,
                state: block_chunk1.midstate(),
            })
        }

        // Once we exhaust version-rolling-space, we start rolling ntime.
        // We can be sure ntime offset is common for all blocks, because `midstate_count`
        // divides the size of range we roll.
        // ntime offset is common for all midstates.
        let ntime_offset = self.get_ntime_offset(current);
        assert_eq!(ntime_offset, self.get_ntime_offset(next - 1));

        let work = Assignment::new(self.job.clone(), midstates, self.job.time() + ntime_offset);
        if self.curr_range.is_exhausted(next) {
            // when the whole version space has been exhausted then mark the generated work as
            // a last one (the next call of this method will return 'Exhausted')
            LoopState::Break(work)
        } else {
            LoopState::Continue(work)
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::job::Bitcoin;
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
        compare_range(
            BIP320_UPPER_BOUND_EXCLUSIVE_INDEX * ROLL_NTIME_SECONDS - 1,
            BIP320_UPPER_BOUND_EXCLUSIVE_INDEX * ROLL_NTIME_SECONDS,
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

    fn get_block_version(job: &Arc<test_utils::TestBlock>, version_index: u32) -> u32 {
        job.version() | (version_index << ii_bitcoin::BIP320_VERSION_SHIFT)
    }

    fn get_ntime(job: &Arc<test_utils::TestBlock>, ntime_index: u32) -> u32 {
        job.time() + ntime_index
    }

    fn make_compound_index(ntime_index: u32, version_index: u32) -> u32 {
        assert!(ntime_index < ROLL_NTIME_SECONDS);
        assert!(version_index <= ii_bitcoin::BIP320_VERSION_MAX);
        ntime_index * BIP320_UPPER_BOUND_EXCLUSIVE_INDEX + version_index
    }

    #[test]
    fn test_ntime_increment() {
        // use first test block for job
        let job = Arc::new(test_utils::TEST_BLOCKS[0]);
        let engine = VersionRolling::new(job.clone(), 1);

        // position ourselves to end of first version range
        const START_VERSION_INDEX: u32 = ii_bitcoin::BIP320_VERSION_MAX;
        const START_NTIME_INDEX: u32 = 0;
        engine.curr_range.curr_index.store(
            make_compound_index(START_NTIME_INDEX, START_VERSION_INDEX),
            Ordering::Relaxed,
        );
        assert!(!engine.is_exhausted());

        match engine.next_work() {
            LoopState::Continue(work) => {
                assert_eq!(
                    get_block_version(&job, START_VERSION_INDEX),
                    work.midstates[0].version
                );
                assert_eq!(get_ntime(&job, START_NTIME_INDEX), work.ntime);
            }
            _ => panic!("expected 'LoopState::Continue'"),
        }
        assert!(!engine.is_exhausted());

        match engine.next_work() {
            LoopState::Continue(work) => {
                assert_eq!(get_block_version(&job, 0), work.midstates[0].version);
                assert_eq!(get_ntime(&job, START_NTIME_INDEX + 1), work.ntime);
            }
            _ => panic!("expected 'LoopState::Continue'"),
        }
        assert!(!engine.is_exhausted());
    }

    #[test]
    fn test_exhausted_work() {
        // use first test block for job
        let job = Arc::new(test_utils::TEST_BLOCKS[0]);
        let engine = VersionRolling::new(job.clone(), 1);

        // modify current version counter to decrease the search space
        // adn test only boundary values
        const START_VERSION_INDEX: u32 = ii_bitcoin::BIP320_VERSION_MAX - 1;
        const START_NTIME_INDEX: u32 = ROLL_NTIME_SECONDS - 1;
        engine.curr_range.curr_index.store(
            make_compound_index(START_NTIME_INDEX, START_VERSION_INDEX),
            Ordering::Relaxed,
        );
        assert!(!engine.is_exhausted());

        match engine.next_work() {
            LoopState::Continue(work) => {
                assert_eq!(
                    get_block_version(&job, START_VERSION_INDEX),
                    work.midstates[0].version
                );
                assert_eq!(get_ntime(&job, START_NTIME_INDEX), work.ntime);
            }
            _ => panic!("expected 'LoopState::Continue'"),
        }
        assert!(!engine.is_exhausted());

        match engine.next_work() {
            LoopState::Break(work) => {
                assert_eq!(
                    get_block_version(&job, ii_bitcoin::BIP320_VERSION_MAX),
                    work.midstates[0].version
                );
                assert_eq!(get_ntime(&job, START_NTIME_INDEX), work.ntime);
            }
            _ => panic!("expected 'LoopState::Break'"),
        }
        assert!(engine.is_exhausted());

        match engine.next_work() {
            LoopState::Exhausted => {}
            _ => panic!("expected 'LoopState::Exhausted'"),
        }
        assert!(engine.is_exhausted());
    }
}
