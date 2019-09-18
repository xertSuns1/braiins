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

//! This module deals with serializing and de-serializing (extended) work_id
//! numbers that go in/out of FPGA bitstream.
//!
//! * `InputTag` is a value attached to work when going into chip (in datasheet
//!   it's called "extended work ID")
//! * `OutputTag` is a value attached to nonce (second word in reply) that goes
//!   out of chip (it's combined `work_id`, `midstate_idx`, `solution_idx` and
//!   checksum)
//!
//! `OutputTag` provides parsing and representation of "solution id" part of
//! chip response read as second word from FPGA FIFO.

use crate::MidstateCount;

#[derive(Debug, Clone)]
pub struct OutputTag {
    pub work_id: usize,
    pub midstate_idx: usize,
    pub solution_idx: usize,
}

#[derive(Debug, Clone)]
pub struct TagManager {
    midstate_count: MidstateCount,
}

impl TagManager {
    /// Bit position where work ID starts in the second word provided by the IP core with mining work
    /// solution
    const ID_OFFSET: usize = 8;
    const ID_LIMIT: usize = 1 << 16;

    pub fn new(midstate_count: MidstateCount) -> Self {
        Self { midstate_count }
    }

    /// Extract fields of "solution id" word into OutputTag
    pub fn parse_output_tag(&self, reg: u32) -> OutputTag {
        let id = (reg as usize >> Self::ID_OFFSET) & (Self::ID_LIMIT - 1);
        OutputTag {
            solution_idx: reg as usize & ((1 << Self::ID_OFFSET) - 1),
            work_id: id >> self.midstate_count.to_bits(),
            midstate_idx: id & self.midstate_count.to_mask(),
        }
    }

    /// Make `work_id` into a `InputTag` suitable for writing in `extended work ID` field when
    /// submitting a job.
    pub fn make_input_tag(&self, work_id: usize, midstate_count: usize) -> u32 {
        // Check number of midstates is correct
        assert_eq!(
            midstate_count,
            self.midstate_count.to_count(),
            "Outgoing work has {} midstates, but miner is configured for {} midstates!",
            midstate_count,
            self.midstate_count.to_count(),
        );
        // Check `work_id` is within range
        assert!(work_id < (Self::ID_LIMIT >> self.midstate_count.to_bits()));
        // Generate `InputTag`
        ((work_id << self.midstate_count.to_bits()) & (Self::ID_LIMIT - 1)) as u32
    }

    pub fn work_id_limit(&self) -> usize {
        Self::ID_LIMIT >> self.midstate_count.to_bits()
    }
}

/// This test verifies correct parsing of mining work solution for all multi-midstate
/// configurations.
/// The solution_word represents the second word of data provided that follows the nonce as
/// provided by the FPGA IP core
#[test]
fn test_output_tag() {
    let solution_word = 0x98123502;
    struct ExpectedSolutionData {
        work_id: usize,
        midstate_idx: usize,
        solution_idx: usize,
        midstate_count: MidstateCount,
    };
    let expected_solution_data = [
        ExpectedSolutionData {
            work_id: 0x1235,
            midstate_idx: 0,
            solution_idx: 2,
            midstate_count: MidstateCount::new(1),
        },
        ExpectedSolutionData {
            work_id: 0x091a,
            midstate_idx: 1,
            solution_idx: 2,
            midstate_count: MidstateCount::new(2),
        },
        ExpectedSolutionData {
            work_id: 0x048d,
            midstate_idx: 1,
            solution_idx: 2,
            midstate_count: MidstateCount::new(4),
        },
    ];
    for (i, expected_solution_data) in expected_solution_data.iter().enumerate() {
        // The midstate configuration (ctrl_reg::MIDSTATE_CNT_W) doesn't implement a debug
        // trait. Therefore, we extract only those parts that can be easily displayed when a
        // test failed.
        let expected_data = (
            expected_solution_data.work_id,
            expected_solution_data.midstate_idx,
            expected_solution_data.solution_idx,
        );
        let output_tag =
            TagManager::new(expected_solution_data.midstate_count).parse_output_tag(solution_word);

        assert_eq!(
            output_tag.work_id, expected_solution_data.work_id,
            "Invalid work ID, iteration: {}, test data: {:#06x?}",
            i, expected_data
        );
        assert_eq!(
            output_tag.midstate_idx, expected_solution_data.midstate_idx,
            "Invalid midstate index, iteration: {}, test data: {:#06x?}",
            i, expected_data
        );
        assert_eq!(
            output_tag.solution_idx, expected_solution_data.solution_idx,
            "Invalid solution index, iteration: {}, test data: {:#06x?}",
            i, expected_data
        );
    }
}

/// Test that input tags get assembled correctly
#[test]
fn test_input_tag() {
    assert_eq!(
        TagManager::new(MidstateCount::new(1)).make_input_tag(0x8765, 1),
        0x8765
    );
    assert_eq!(
        TagManager::new(MidstateCount::new(2)).make_input_tag(0x43b2, 2),
        0x8764
    );
    assert_eq!(
        TagManager::new(MidstateCount::new(4)).make_input_tag(0x21d9, 4),
        0x8764
    );
}

/// Test that input tags that would overflow the 16bit field will panic
#[test]
#[should_panic]
fn test_input_tag_fail() {
    TagManager::new(MidstateCount::new(2)).make_input_tag(0x8765, 2);
}

/// Test that midstate count and tagged work midstat count match
#[test]
#[should_panic]
fn test_input_tag_fail2() {
    TagManager::new(MidstateCount::new(1)).make_input_tag(0x8765, 4);
}
