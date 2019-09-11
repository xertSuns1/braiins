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
use packed_struct_codegen::PackedStruct;
use packed_struct_codegen::{PrimitiveEnum_u16, PrimitiveEnum_u8};

use std::mem::size_of;

use super::error::{self, ErrorKind};

pub const GET_ADDRESS_REG: u8 = 0x00;
pub const PLL_PARAM_REG: u8 = 0x0c;
#[allow(dead_code)]
pub const HASH_COUNTING_REG: u8 = 0x14;
#[allow(dead_code)]
pub const TICKET_MASK_REG: u8 = 0x18;
pub const MISC_CONTROL_REG: u8 = 0x1c;

/// Maximum supported baud rate clock divisor
const MAX_BAUD_CLOCK_DIV: usize = 26;

/// Basic divisor of the clock speed when calculating the value for the baud register
pub const CHIP_OSC_CLK_BASE_BAUD_DIV: usize = 8;

/// How many cores are on the chip
pub const NUM_CORES_ON_CHIP: usize = 114;

/// Control or work command layout
#[derive(PackedStruct, Debug)]
#[packed_struct(size_bytes = "1", bit_numbering = "lsb0")]
pub struct Cmd {
    #[packed_field(bits = "0:3")]
    code: Integer<u8, packed_bits::Bits4>,
    #[packed_field(bits = "4")]
    to_all: bool,
    #[packed_field(bits = "5:7", ty = "enum")]
    cmd_type: CmdType,
}
impl Cmd {
    fn new(code: u8, to_all: bool) -> Self {
        Self {
            code: code.into(),
            to_all,
            cmd_type: CmdType::VilCtlCmd,
        }
    }
}

/// Command types
#[derive(PrimitiveEnum_u8, Clone, Copy, Debug, PartialEq)]
enum CmdType {
    /// Control command for the chip
    VilCtlCmd = 0x02,
}

#[derive(PackedStruct, Debug)]
pub struct CmdHeader {
    #[packed_field(element_size_bytes = "1")]
    cmd: Cmd,
    length: u8,
    chip_address: u8,
}
impl CmdHeader {
    ///
    /// * `length` - size of the command excluding checksum
    /// * `checksum_size` - Size of checksum needs to be known as it is accounted in the length
    /// field
    fn new(cmd: Cmd, length: usize, chip_address: u8, checksum_size: usize) -> Self {
        Self {
            cmd,
            length: (length + checksum_size) as u8,
            chip_address,
        }
    }

    /// Helper builder for control commands
    /// Control commands CRC5 checksum that fits into 1 byte
    /// * `length` - length of the command without checksum
    fn new_ctl_cmd_header(cmd: Cmd, length: usize, chip_address: u8) -> Self {
        Self::new(cmd, length, chip_address, size_of::<u8>())
    }

    #[inline]
    #[allow(dead_code)]
    /// Helper method - when streaming multiple commands to eliminate the need to create a new
    /// command instance every time, we allow modifying the chip address
    pub fn set_chip_address(&mut self, addr: u8) {
        self.chip_address = addr;
    }
}

/// Sets configuration register
#[derive(PackedStruct, Debug)]
#[packed_struct(endian = "lsb")]
pub struct SetConfigCmd {
    #[packed_field(element_size_bytes = "3")]
    pub header: CmdHeader,
    register: u8,
    value: u32,
}

impl SetConfigCmd {
    pub fn new(chip_address: u8, to_all: bool, register: u8, value: u32) -> Self {
        let cmd = Cmd::new(0x08, to_all);
        // payload consists of 1 byte register address and 4 byte value
        let header = CmdHeader::new_ctl_cmd_header(cmd, Self::packed_bytes(), chip_address);
        Self {
            header,
            register,
            value,
        }
    }
}

#[derive(PackedStruct, Debug)]
#[packed_struct(endian = "lsb")]
pub struct GetStatusCmd {
    #[packed_field(element_size_bytes = "3")]
    header: CmdHeader,
    register: u8,
}

impl GetStatusCmd {
    pub fn new(chip_address: u8, to_all: bool, register: u8) -> Self {
        let cmd = Cmd::new(0x04, to_all);
        let header = CmdHeader::new_ctl_cmd_header(cmd, Self::packed_bytes(), chip_address);
        Self { header, register }
    }
}

#[derive(PackedStruct, Debug)]
#[packed_struct(endian = "lsb")]
pub struct SetChipAddressCmd {
    #[packed_field(element_size_bytes = "3")]
    pub header: CmdHeader,
    _reserved: u8,
}

impl SetChipAddressCmd {
    pub fn new(chip_address: u8) -> Self {
        let cmd = Cmd::new(0x01, false);
        let header = CmdHeader::new_ctl_cmd_header(cmd, Self::packed_bytes(), chip_address);
        Self {
            header,
            _reserved: 0,
        }
    }
}

#[derive(PackedStruct, Debug)]
#[packed_struct(endian = "lsb")]
pub struct InactivateFromChainCmd {
    #[packed_field(element_size_bytes = "3")]
    header: CmdHeader,
    _reserved: u8,
}

impl InactivateFromChainCmd {
    pub fn new() -> Self {
        let cmd = Cmd::new(0x05, true);
        let header = CmdHeader::new_ctl_cmd_header(cmd, Self::packed_bytes(), 0);
        Self {
            header,
            _reserved: 0,
        }
    }
}

#[derive(PackedStruct, Default, Debug)]
#[packed_struct(endian = "lsb", size_bytes = "6")]
pub struct GetAddressReg {
    #[packed_field(endian = "msb", ty = "enum", element_size_bytes = "2")]
    pub chip_rev: ChipRev,
    _reserved1: u8,
    pub addr: u8,
    _reserved2: [u8; 2],
}

/// Describes recognized chip revisions
#[derive(PrimitiveEnum_u16, Clone, Copy, Debug, PartialEq)]
pub enum ChipRev {
    Bm1387 = 0x1387,
}

impl Default for ChipRev {
    fn default() -> ChipRev {
        ChipRev::Bm1387
    }
}

/// This register represents ASIC difficulty
///
/// The chip will provide only solutions that are <= target based on this difficulty
#[derive(PackedStruct, Debug)]
#[packed_struct(size_bytes = "4", endian = "lsb")]
pub struct TicketMaskReg {
    /// stores difficulty - 1
    diff: u32,
}

impl TicketMaskReg {
    /// Builds ticket mask register instance and verifies the specified difficulty is correct
    pub fn new(diff: u32) -> error::Result<Self> {
        if diff == 0 {
            Err(ErrorKind::General(format!(
                "Asic difficulty must be at least 1!",
            )))?
        }
        Ok(Self { diff: diff - 1 })
    }
}

/// Converts the register value into a word accepted by the FPGA
impl Into<u32> for TicketMaskReg {
    fn into(self) -> u32 {
        let reg_bytes = self.pack();
        // packed struct already took care of endianess conversion, we just need the canonical
        // value as u32
        u32::from_be_bytes(reg_bytes)
    }
}

/// Core register that configures the most important aspects of the mining chip like:
///
/// - baud rate/communication speed
/// - multi-midstate processing (AsicBoost)
///
/// All the fields below have been identified in bmminer-mix sources. Meaning of some of them may
/// still be a bit unclear.
///
/// TODO: research set_baud_with_addr() in bmminer-mix as there seems to be some magic setting
/// I2C interface of the chip or something like that
#[derive(PackedStruct, Debug)]
#[packed_struct(bit_numbering = "lsb0", size_bytes = "4", endian = "msb")]
pub struct MiscCtrlReg {
    /// Exact meaning of this field is unknown, when setting baud rate, it is 0, when
    /// initializing the chain it is 1
    /// bit 6
    #[packed_field(bits = "6")]
    pub not_set_baud: bool,

    /// Invert clock pin -> used on S9's
    /// bit 13
    #[packed_field(bits = "13")]
    pub inv_clock: bool,

    /// baudrate divisor - maximum divisor is 26. To calculate the divisor:
    /// baud_div = min(OSC/8*baud - 1, 26)
    /// Oscillator frequency is 25 MHz
    /// bit 20:16
    #[packed_field(bits = "20:16")]
    pub baud_div: Integer<u8, packed_bits::Bits5>,

    /// This field causes all blocks of the hashing chip to ignore any incoming
    /// work and allows enabling the blocks one-by-one when a mining work with bit[0] set to 1
    /// arrives
    /// bit 23
    #[packed_field(bits = "23")]
    pub gate_block: bool,

    /// Enable multi midstate processing = "AsicBoost"
    /// bit 31
    #[packed_field(bits = "31")]
    pub mmen: bool,
}

impl MiscCtrlReg {
    /// Builds register instance and sanity checks the divisor for the baud rate generator
    pub fn new(
        not_set_baud: bool,
        inv_clock: bool,
        baud_div: usize,
        gate_block: bool,
        mmen: bool,
    ) -> error::Result<Self> {
        if baud_div > MAX_BAUD_CLOCK_DIV {
            Err(ErrorKind::BaudRate(format!(
                "divisor {} is out of range, maximum allowed is {}",
                baud_div, MAX_BAUD_CLOCK_DIV
            )))?
        }
        Ok(Self {
            not_set_baud,
            inv_clock,
            baud_div: (baud_div as u8).into(),
            gate_block,
            mmen,
        })
    }
}

impl Into<u32> for MiscCtrlReg {
    fn into(self) -> u32 {
        let reg_bytes = self.pack();
        u32::from_be_bytes(reg_bytes)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    /// Builds a sample set_config command (here the PLL register @ 0x0c with a value of
    /// 0x21026800 that corresponds to
    /// and verifies correct serialization
    fn build_set_config_cmd() {
        let cmd = SetConfigCmd::new(0x24, false, PLL_PARAM_REG, 0x21026800);
        let expected_cmd_with_padding = [0x48u8, 0x09, 0x24, PLL_PARAM_REG, 0x00, 0x68, 0x02, 0x21];
        //        let expected_cmd_with_padding = u8_as_fpga_slice(&expected_cmd_with_padding);
        let cmd_bytes = cmd.pack();
        assert_eq!(
            cmd_bytes, expected_cmd_with_padding,
            "Incorrectly composed command:{:#04x?} sliced view: {:#04x?} expected view: \
             {:#04x?}",
            cmd, cmd_bytes, expected_cmd_with_padding
        );
    }

    #[test]
    // Verify serialization of SetConfig(TICKET_MASK(0x3f)) command
    fn build_set_config_ticket_mask() {
        let reg = TicketMaskReg::new(64).expect("Cannot build difficulty register");
        let cmd = SetConfigCmd::new(0x00, true, TICKET_MASK_REG, reg.into());
        let expected_cmd_with_padding = [0x58u8, 0x09, 0x00, 0x18, 0x00, 0x00, 0x00, 0x3f];
        let cmd_bytes = cmd.pack();
        assert_eq!(cmd_bytes, expected_cmd_with_padding);
    }

    #[test]
    /// Builds a get status command to read chip address of all chips
    fn build_get_status_cmd() {
        let cmd = GetStatusCmd::new(0x00, true, GET_ADDRESS_REG);
        let expected_cmd_with_padding = [0x54u8, 0x05, 0x00, 0x00];

        let cmd_bytes = cmd.pack();
        assert_eq!(
            cmd_bytes, expected_cmd_with_padding,
            "Incorrectly composed command:{:#04x?} sliced view: {:#04x?} expected view: \
             {:#04x?}",
            cmd, cmd_bytes, expected_cmd_with_padding
        );
    }

    #[test]
    fn build_inactivate_from_chain_cmd() {
        let cmd = InactivateFromChainCmd::new();
        let expected_cmd_with_padding = [0x55u8, 0x05, 0x00, 0x00];

        let cmd_bytes = cmd.pack();
        assert_eq!(
            cmd_bytes, expected_cmd_with_padding,
            "Incorrectly composed command:{:#04x?} sliced view: {:#04x?} expected view: \
             {:#04x?}",
            cmd, cmd_bytes, expected_cmd_with_padding
        );
    }

    #[test]
    fn build_set_chip_address_cmd() {
        let cmd = SetChipAddressCmd::new(0x04);
        let expected_cmd_with_padding = [0x41u8, 0x05, 0x04, 0x00];

        let cmd_bytes = cmd.pack();
        assert_eq!(
            cmd_bytes, expected_cmd_with_padding,
            "Incorrectly composed command:{:#04x?} sliced view: {:#04x?} expected view: \
             {:#04x?}",
            cmd, cmd_bytes, expected_cmd_with_padding
        );
    }

    #[test]
    fn build_chip_addr_reg() {
        let reg = GetAddressReg {
            chip_rev: ChipRev::Bm1387,
            _reserved1: 0x90,
            addr: 0x00,
            ..Default::default()
        };
        let expected_reg = [0x13u8, 0x87, 0x90, 0x00, 0x00, 0x00];

        let reg_bytes = reg.pack();
        assert_eq!(
            reg_bytes, expected_reg,
            "Incorrectly composed register:{:#04x?} sliced view: {:#04x?} expected view: \
             {:#04x?}",
            reg, reg_bytes, expected_reg
        );
    }

    #[test]
    fn test_broken_chip_addr_value() {
        // intentionally specify incorrect/unsupported chip version
        let broken_reg_bytes = [0x13u8, 0x86, 0x90, 0x04, 0x00, 0x00];
        let reg = GetAddressReg::unpack_from_slice(&broken_reg_bytes);
        assert!(
            reg.is_err(),
            "Unpacking should have failed due to incompatible chip version \
             parsed: {:?}, sliced view: {:#04x?}",
            reg,
            broken_reg_bytes
        );
    }

    #[test]
    fn build_misc_control_reg() {
        let reg = MiscCtrlReg {
            not_set_baud: true,
            inv_clock: true,
            baud_div: 26.into(),
            gate_block: true,
            mmen: true,
        };
        let expected_reg_msb = [0x80u8, 0x9a, 0x20, 0x40];

        let reg_bytes = reg.pack();

        assert_eq!(
            reg_bytes, expected_reg_msb,
            "Incorrectly composed register:{:#04x?} sliced view: {:#04x?} expected view: \
             {:#04x?}",
            reg, reg_bytes, expected_reg_msb
        );
    }

    #[test]
    fn test_misc_control_reg_to_u32() {
        let reg = MiscCtrlReg {
            not_set_baud: true,
            inv_clock: true,
            baud_div: 26.into(),
            gate_block: true,
            mmen: true,
        };
        let expected_reg_value = 0x809a2040u32;
        let reg_value: u32 = reg.into();
        assert_eq!(
            reg_value, expected_reg_value,
            "Misc Control Register 32-bit value  doesn't match: V:{:#010x} E:{:#010x}",
            reg_value, expected_reg_value
        );
    }

    #[test]
    fn test_invalid_ticket_mask_reg() {
        let res = TicketMaskReg::new(0);
        assert_eq!(res.is_ok(), false, "Diff 0 should be reported as error!");
    }

    #[test]
    fn test_ticket_mask_reg_to_u32() {
        let reg = TicketMaskReg::new(64).expect("Cannot build difficulty register");

        let expected_reg_value = 0x3f00_0000u32;
        let reg_value: u32 = reg.into();
        assert_eq!(
            reg_value, expected_reg_value,
            "Ticket mask register 32-bit value  doesn't match: V:{:#010x} E:{:#010x}",
            reg_value, expected_reg_value
        );
    }

}
