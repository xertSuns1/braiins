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

use ii_bitcoin::HashTrait;

use std::convert::TryInto;
use std::fmt::Debug;
use std::mem;

use downcast_rs::{impl_downcast, Downcast};

/// Represents interface for Bitcoin job with access to block header from which the new work will be
/// generated. The trait is bound to Downcast which enables connect work solution with original job
/// and hide protocol specific details.
pub trait Bitcoin: Debug + Downcast + Send + Sync {
    /// Original version field that reflects the current network consensus
    fn version(&self) -> u32;
    /// Bit-mask with general purpose bits which can be freely manipulated (specified by BIP320)
    fn version_mask(&self) -> u32;
    /// Double SHA256 hash of the previous block header
    fn previous_hash(&self) -> &ii_bitcoin::DHash;
    /// Double SHA256 hash based on all of the transactions in the block
    fn merkle_root(&self) -> &ii_bitcoin::DHash;
    /// Current block timestamp as seconds since 1970-01-01T00:00 UTC
    fn time(&self) -> u32;
    /// Maximal timestamp for current block as seconds since 1970-01-01T00:00 UTC
    fn max_time(&self) -> u32 {
        self.time()
    }
    /// Current target in compact format (network difficulty)
    /// https://en.bitcoin.it/wiki/Difficulty
    fn bits(&self) -> u32;
    /// Checks if job is still valid for mining
    fn is_valid(&self) -> bool;

    /// Extract least-significant word of merkle root that goes to chunk2 of SHA256
    /// The word is interpreted as a little endian number.
    #[inline]
    fn merkle_root_tail(&self) -> u32 {
        let merkle_root = self.merkle_root().into_inner();
        u32::from_le_bytes(
            merkle_root[merkle_root.len() - mem::size_of::<u32>()..]
                .try_into()
                .expect("slice with incorrect length"),
        )
    }
}
impl_downcast!(Bitcoin);
