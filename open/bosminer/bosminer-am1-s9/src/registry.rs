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

use crate::work;

/// Maximum length of pending work list corresponds with the work ID range supported by the FPGA
const MAX_WORK_LIST_COUNT: usize = 65536;

/// Mining registry item contains work and solutions
#[derive(Clone)]
pub struct MiningWorkRegistryItem {
    work: work::Assignment,
    /// Each slot in the vector is associated with particular solution index as reported by
    /// the chips. Generally, hash board may fail to send a preceding solution due to
    /// corrupted communication frames. Therefore, each solution slot is optional.
    solutions: std::vec::Vec<work::Solution>,
}

impl MiningWorkRegistryItem {
    /// Associates a specified solution with mining work, accounts for duplicates and nonce
    /// mismatches
    /// * `solution` - solution to be inserted
    /// * `solution_idx` - each work may have multiple valid solutions, this index denotes its
    /// order. The index is reported by the hashing chip
    pub fn insert_solution(&mut self, new_solution: work::Solution) -> InsertSolutionStatus {
        let mut status = InsertSolutionStatus {
            duplicate: false,
            mismatched_nonce: false,
            unique_solution: None,
        };
        // scan the current solutions and detect a duplicate
        let matching_solution = self
            .solutions
            .iter()
            .find(|solution| solution.nonce == new_solution.nonce);
        if matching_solution.is_none() {
            // At this point, we know such solution has not been received yet. If it is valid (no
            // hardware error detected == meets the target), it can be appended to the solution list
            // for this work item
            // TODO: call the evaluator for the solution
            self.solutions.push(new_solution.clone());
        } else {
            // now we now it's a duplicate, but we return it anyway
            status.duplicate = true;
        }

        // report the unique solution via status
        status.unique_solution = Some(work::UniqueSolution::new(
            self.work.clone(),
            new_solution,
            None,
        ));
        status
    }
}

/// Helper container for the status after inserting the solution
#[derive(Clone)]
pub struct InsertSolutionStatus {
    /// Nonce of the solution at a given index doesn't match the existing nonce
    pub mismatched_nonce: bool,
    /// Solution is duplicate (given MiningWorkRegistryItem) already has it
    pub duplicate: bool,
    /// actual solution (defined if the above 2 are false)
    /// TODO: rename `unique_solution` to solution
    pub unique_solution: Option<work::UniqueSolution>,
}

/// Simple mining work registry that stores each work in a slot denoted by its work ID.
///
/// The slots are handled in circular fashion, when storing new work, any work older than
/// MAX_WORK_LIST_COUNT/2 sequence ID's in the past is to be retired.
pub struct MiningWorkRegistry {
    /// Current pending work list Each work item has a list of associated work solutions
    pending_work_list: std::vec::Vec<Option<MiningWorkRegistryItem>>,
    /// Keeps track of the ID, so that we can identify stale solutions
    last_work_id: Option<usize>,
    /// Number of midstates by which the `last_work_id` increments
    midstate_count: usize,
}

impl MiningWorkRegistry {
    pub fn new(midstate_count: usize) -> Self {
        Self {
            pending_work_list: vec![None; MAX_WORK_LIST_COUNT],
            last_work_id: None,
            midstate_count,
        }
    }

    /// Helper method that performs modulo subtraction on the indices of the vector.
    /// This enables circular buffer arithmetic
    #[inline]
    fn index_sub(x: usize, y: usize) -> usize {
        x.wrapping_sub(y).wrapping_add(MAX_WORK_LIST_COUNT) % MAX_WORK_LIST_COUNT
    }

    /// Stores new work in the registry and retires (removes) any stale work with ID
    /// older than 1/2 of MAX_WORK_LIST_COUNT
    /// * `id` - identifies the work
    /// * `work` - new work to be stored
    pub fn store_work(&mut self, id: usize, work: work::Assignment) {
        // The slot must be empty
        assert!(
            self.pending_work_list[id].is_none(),
            "Slot at index {} is not empty",
            id
        );
        // and the new work has to be sequenced
        if let Some(last_work_id) = self.last_work_id {
            assert_eq!(
                Self::index_sub(id, last_work_id),
                self.midstate_count,
                "Work id is out of sequence {}",
                id
            )
        }

        self.last_work_id = Some(id);

        self.pending_work_list[id] = Some(MiningWorkRegistryItem {
            work,
            solutions: std::vec::Vec::new(),
        });

        // retire old work that is not expected to have any solution => work with ID older than
        // MAX_WORK_LIST_COUNT/2 is marked obsolete
        let retire_id = Self::index_sub(id, MAX_WORK_LIST_COUNT / 2);

        self.pending_work_list[retire_id] = None;
    }

    pub fn find_work(&mut self, id: usize) -> &mut Option<MiningWorkRegistryItem> {
        &mut self.pending_work_list[id]
    }
}

#[cfg(test)]
mod test {
    use super::super::null_work;
    use super::*;

    #[test]
    fn test_store_work_start() {
        let mut registry = MiningWorkRegistry::new(1);
        let work = null_work::prepare(0);

        registry.store_work(0, work);
    }

    #[test]
    #[should_panic]
    fn test_store_work_out_of_sequence_work_id() {
        let mut registry = MiningWorkRegistry::new(1);
        let work1 = null_work::prepare(0);
        let work2 = null_work::prepare(1);
        // store initial work
        registry.store_work(0, work1);
        // this should trigger a panic
        registry.store_work(2, work2);
    }

    #[test]
    fn test_store_work_id_two_midstates() {
        let mut registry = MiningWorkRegistry::new(2);
        let work1 = null_work::prepare(0);
        let work2 = null_work::prepare(1);
        // store initial work
        registry.store_work(0, work1);
        // should not panic, we increment by # of midstates
        registry.store_work(2, work2);
    }

    #[test]
    fn test_store_work_retiring() {
        let mut registry = MiningWorkRegistry::new(1);
        // after exhausting the full work list count, the first half of the slots must be retired
        for id in 0..MAX_WORK_LIST_COUNT {
            let work = null_work::prepare(id as u64);
            registry.store_work(id, work);
        }
        // verify the first half being empty
        for id in 0..MAX_WORK_LIST_COUNT / 2 {
            assert!(
                registry.pending_work_list[0].is_none(),
                "Work at id {} was expected to be retired!",
                id
            );
        }
        // verify the second half being non-empty
        for id in MAX_WORK_LIST_COUNT / 2..MAX_WORK_LIST_COUNT {
            assert!(
                registry.pending_work_list[id].is_some(),
                "Work at id {} was expected to be defined!",
                id
            );
        }

        // store one more item should retire work at index MAX_WORK_LIST_COUNT/2
        let retire_idx_half = MAX_WORK_LIST_COUNT / 2;
        registry.store_work(0, null_work::prepare(0));
        assert!(
            registry.pending_work_list[retire_idx_half].is_none(),
            "Work at {} was expected to be retired (after overwriting idx 0)",
            retire_idx_half
        );
    }
}
