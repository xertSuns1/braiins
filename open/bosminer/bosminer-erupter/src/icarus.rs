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

//! Provides Icarus hashing chip driver

use packed_struct::prelude::*;
use packed_struct_codegen::PackedStruct;

use std::mem::size_of;

use lazy_static::lazy_static;

lazy_static! {
    /// ASIC difficulty is 1 and cannot be changed
    pub static ref ASIC_TARGET: ii_bitcoin::Target = Default::default();
}

// time for computation one double hash and target comparison in seconds
pub const HASH_TIME_S: f64 = 0.0000000029761;

/// Time needed for iteration of the whole search space in milliseconds
pub const FULL_NONCE_TIME_MS: f64 = (HASH_TIME_S * (0xffffffffu64 + 1u64) as f64) * 1000f64;
/// Size of work structure required by the chip
pub const WORK_PAYLOAD_SIZE: usize = 64;

/// Icarus work payload containing all information for finding Bitcoin block header nonce
#[derive(PackedStruct, Debug, Clone, Copy, Default)]
#[packed_struct(endian = "lsb")]
pub struct WorkPayload {
    /// Internal state of SHA256 after processing the first chunk (32 bytes)
    midstate: [u32; 8],
    /// The following fields are used by some variants of the chip and are not documented
    check: u8,
    data: u8,
    cmd: u8,
    prefix: u8,
    unused: [u8; 15],
    id: u8,
    /// Current target in compact format (network difficulty)
    /// https://en.bitcoin.it/wiki/Difficulty
    pub bits: u32,
    /// Current block timestamp as seconds since 1970-01-01T00:00 UTC
    pub time: u32,
    /// Least-significant word of merkle root that goes to chunk2 of SHA256
    pub merkle_tail: u32,
}

impl WorkPayload {
    pub fn new(midstate: &ii_bitcoin::Midstate, merkle_tail: u32, time: u32, bits: u32) -> Self {
        // midstate 32bit words are stored in array in a reverse order
        let mut midstate_words = [0u32; ii_bitcoin::SHA256_DIGEST_SIZE / size_of::<u32>()];
        for (i, word) in midstate.words().rev().enumerate() {
            midstate_words[i] = word;
        }

        Self {
            midstate: midstate_words,
            bits,
            time,
            merkle_tail,
            ..Default::default()
        }
    }

    /// Get binary representation of Bitcoin block header
    #[inline]
    pub fn into_bytes(self) -> [u8; WORK_PAYLOAD_SIZE] {
        self.pack()
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use bosminer::job::Bitcoin;
    use bosminer::test_utils;

    #[test]
    fn test_work_payload() {
        for block in test_utils::TEST_BLOCKS.iter() {
            let work = WorkPayload::new(
                &block.midstate,
                block.merkle_root_tail(),
                block.time(),
                block.bits(),
            );

            // check binary representation of Icarus work header
            assert_eq!(block.icarus_bytes[..], work.into_bytes()[..]);
        }
    }
}
