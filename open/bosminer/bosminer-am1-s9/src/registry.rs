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

use crate::Solution;

use bosminer::work;
use std::iter::Iterator;

/// Mining registry item contains work and solutions
#[derive(Clone)]
pub struct WorkRegistryItem {
    work: work::Assignment,
    /// Each slot in the vector is associated with particular solution index as reported by
    /// the chips.
    solutions: std::vec::Vec<Solution>,
}

impl WorkRegistryItem {
    /// Associates a specified solution with mining work, accounts for duplicates and nonce
    /// mismatches
    /// * `solution` - solution to be inserted
    /// * `solution_idx` - each work may have multiple valid solutions, this index denotes its
    /// order. The index is reported by the hashing chip
    pub fn insert_solution(&mut self, new_solution: Solution) -> InsertSolutionStatus {
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
        status.unique_solution = Some(work::Solution::new(self.work.clone(), new_solution, None));
        status
    }
}

/// Helper container for the status after inserting the solution
#[derive(Clone)]
pub struct InsertSolutionStatus {
    /// Nonce of the solution at a given index doesn't match the existing nonce
    pub mismatched_nonce: bool,
    /// Solution is duplicate (given WorkRegistryItem) already has it
    pub duplicate: bool,
    /// actual solution (defined if the above 2 are false)
    /// TODO: rename `unique_solution` to solution
    pub unique_solution: Option<work::Solution>,
}

/// Simple work registry with `work_id` allocator
///
/// Registry is responsible for associating `work` with `work_id` and managing
/// this relation for the lifetime of the work.
/// The `work_id` is allocated in circular fashion from the range `[0, registry_size - 1]`.
/// The lifetime of work is set to `registry_size / 2` - after this much new work
/// has been inserted after some particular work, the work is retired.
///
/// The idea behind this registry is that we manage `registry_size` of slots and
/// we assign work to them (under `work_id` we generate for each inserted work), but
/// we always keep at least `registry_size / 2` slots free, so that we can detect
/// stale work.
pub struct WorkRegistry {
    /// Number of elements in registry. Determines `work_id` range
    registry_size: usize,
    /// Next id that is to be assigned to work, this increases modulo `registry_size`
    next_work_id: usize,
    /// Current pending work list Each work item has a list of associated work solutions
    pending_work_list: std::vec::Vec<Option<WorkRegistryItem>>,
}

impl WorkRegistry {
    /// Create new registry with `registry_size` slots
    pub fn new(registry_size: usize) -> Self {
        Self {
            registry_size,
            next_work_id: 0,
            pending_work_list: vec![None; registry_size],
        }
    }

    /// Allocate next `work_id`. IDs are assigned in circular fashion.
    /// This function is internal to the registry
    fn alloc_next_work_id(&mut self) -> usize {
        let work_id = self.next_work_id;

        // advance next work_id and wrap it manually
        self.next_work_id = (work_id + 1) % self.registry_size;

        // return next work id
        work_id
    }

    /// Store new work to work registry and generate `work_id` for it
    /// As a side effect, retire stale work.
    /// Returns: new `work_id`
    pub fn store_work(&mut self, work: work::Assignment) -> usize {
        let work_id = self.alloc_next_work_id();

        // retire stale work
        let retire_id = (work_id + self.registry_size / 2) % self.registry_size;
        self.pending_work_list[retire_id] = None;

        // put new work into registry
        self.pending_work_list[work_id] = Some(WorkRegistryItem {
            work,
            solutions: std::vec::Vec::new(),
        });

        // return assigned work id
        work_id
    }

    /// Look-up work id
    pub fn find_work(&mut self, work_id: usize) -> &mut Option<WorkRegistryItem> {
        assert!(work_id < self.registry_size);
        &mut self.pending_work_list[work_id]
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::null_work;

    /// Test that it's possible to store work
    #[test]
    fn test_store_work() {
        let mut registry = WorkRegistry::new(4);
        let work1 = null_work::prepare(0);
        let work2 = null_work::prepare(1);

        assert_eq!(registry.store_work(work1), 0);
        assert_eq!(registry.store_work(work2), 1);
        assert!(registry.find_work(0).is_some());
        assert!(registry.find_work(1).is_some());
        assert!(registry.find_work(2).is_none());
    }

    /// Test that old work retires correctly and in order
    #[test]
    fn test_store_work_retiring() {
        const REGISTRY_SIZE: usize = 8;
        const NUM_WORK_ITEMS: usize = REGISTRY_SIZE * 2 + REGISTRY_SIZE / 2 + 1;
        let mut registry = WorkRegistry::new(REGISTRY_SIZE);

        // we store more than REGISTRY_SIZE items so it has to roll over
        for i in 0..NUM_WORK_ITEMS {
            let work = null_work::prepare(i as u64);
            assert_eq!(registry.store_work(work), i % REGISTRY_SIZE);
        }

        // verify that half of registry is empty, half used
        let num_used_slots: usize = registry
            .pending_work_list
            .iter()
            .map(|x| x.is_some() as usize)
            .sum();
        assert_eq!(num_used_slots, REGISTRY_SIZE / 2);

        // verify which items are present
        for i in (NUM_WORK_ITEMS - REGISTRY_SIZE / 2)..NUM_WORK_ITEMS {
            assert!(registry.find_work(i % REGISTRY_SIZE).is_some());
        }
    }

    /// Test that `work_id` counter wraps around
    #[test]
    fn test_work_id_wrap_around() {
        const REGISTRY_SIZE: usize = 4;
        let mut registry = WorkRegistry::new(REGISTRY_SIZE);
        let work = null_work::prepare(0);
        assert_eq!(registry.store_work(work.clone()), 0);
        assert_eq!(registry.store_work(work.clone()), 1);
        assert_eq!(registry.store_work(work.clone()), 2);
        assert_eq!(registry.store_work(work.clone()), 3);
        assert_eq!(registry.store_work(work.clone()), 0);
    }
}
