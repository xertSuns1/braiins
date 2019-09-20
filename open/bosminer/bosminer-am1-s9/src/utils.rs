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

use packed_struct::prelude::*;

/// Just an util trait so that we can pack/unpack directly to registers
pub trait PackedRegister: Sized {
    fn from_reg(reg: u32) -> Result<Self, PackingError>;
    fn to_reg(&self) -> u32;
}

impl<T> PackedRegister for T
where
    T: PackedStruct<[u8; 4]>,
{
    /// Take register and unpack (as big endian)
    fn from_reg(reg: u32) -> Result<Self, PackingError> {
        Self::unpack(&u32::to_be_bytes(reg))
    }
    /// Pack into big-endian register
    fn to_reg(&self) -> u32 {
        u32::from_be_bytes(self.pack())
    }
}
