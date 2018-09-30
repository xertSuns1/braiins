use core;
use packed_struct::prelude::*;

use std::io;
use std::mem;
use std::mem::size_of;

pub const GET_ADDRESS_REG: u8 = 0x00;
pub const PLL_PARAM_REG: u8 = 0x0c;
pub const HASH_COUNTING_REG: u8 = 0x14;
pub const TICKET_MASK_REG: u8 = 0x14;
pub const MISC_CONTROL_REG: u8 = 0x1c;

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
    /// Helper method - when streaming multiple commands to eliminate the need to create a new
    /// command instance every time, we allow modifying the chip address
    pub fn set_chip_address(&mut self, addr: u8) {
        self.chip_address = addr;
    }
}

//#[derive(PackedStruct)]
///// Represents an command sent to the chip via FPGA
//struct FpgaCmd<T, U> {
//    /// actual payload to be sent
//    payload: T,
//    /// Padding required by the FPGA, the exact type depends on the required format
//    fpga_padding: U,
//}
//
//impl<T, U> FpgaCmd<T, U>
//where
//    U: std::convert::From<u8>,
//{
//    fn new(payload: T) -> Self {
//        Self {
//            payload,
//            fpga_padding: U::from(0),
//        }
//    }
//
//    /// Provides slice view of the command that can be sent down the stream
//    ///
//    /// The slice consists of elements of U type. Typically the FPGA accepts data in multiples of
//    /// U size. The slice contains necessary padding.
//    ///
//    /// TODO research whether there some standard trait that should be implemented instead of this
//    /// method
//    ///
//    /// Example:
//    /// ```rust
//    /// let byte_view = self.as_slice()
//    /// ```
//    fn as_fpga_truncated_slice(&self, packed_bytes: &[u8]) -> &[u32] {
//        unsafe {
//            core::slice::from_raw_parts(packed_bytes.as_ptr() as *const u32,
//                                        self.packed_bytes() / size_of::<u32>())
//        }
//    }
//}
/// Custom debug due to the fact that we are printing out a packed structure. Specifically, it is
/// forbidden to borrow potentially unaligned fields (see https://github
/// .com/rust-lang/rust/issues/46043)
//impl<T, U> std::fmt::Debug for FpgaCmd<T, U>
//where
//    T: std::fmt::Debug + std::marker::Copy,
//{
//    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
//        // we have to copy the payload as it may be unaligned
//        let payload = self.payload;
//
//        f.debug_struct("FpgaCmd")
//            .field("payload", &payload)
//            .finish()
//    }
//}

//struct CtlCmd<T> {
//    header: CmdHeader,
//    payload: T,
//}
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

#[derive(PackedStruct, Debug)]
#[packed_struct(endian = "lsb")]
pub struct GetAddressReg {
    #[packed_field(endian = "msb", ty = "enum", element_size_bytes = "2")]
    pub chip_rev: ChipRev,
    _reserved1: u8,
    pub addr: u8,
}

#[derive(PrimitiveEnum_u16, Clone, Copy, Debug, PartialEq)]
/// Command types
pub enum ChipRev {
    /// Control command for the chip
    Bm1387 = 0x1387,
}
//0x40, 0x20, 0x9a, 0x80
//#[packed_struct(size_bytes = "1", bit_numbering = "lsb0")]
///// Control or work command layout
//pub struct Cmd {
//    #[packed_field(bits = "0:3")]
//    code: Integer<u8, packed_bits::Bits4>,
//    #[packed_field(bits = "4")]

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
#[packed_struct(bit_numbering = "lsb0", size_bytes = "4", endian = "lsb")]
pub struct MiscCtrlReg {
    /// Exact meaning of this field is unknown, when setting baud rate, it is 0, when
    /// initializing the chain it is 1
    /// bit 6
    #[packed_field(bits = "30")]
    pub not_set_baud: bool,

    /// Invert clock pin -> used on S9's
    /// bit 13
    #[packed_field(bits = "21")]
    pub inv_clock: bool,

    /// baudrate divisor - maximum divisor is 26. To calculate the divisor:
    /// baud_div = min(OSC/8*baud - 1, 26)
    /// Oscillator frequency is 25 MHz
    /// bit 20:16
    #[packed_field(bits = "12:8")]
    pub baud_div: Integer<u8, packed_bits::Bits5>,

    /// This field most probably causes the core to
    /// bit 23
    #[packed_field(bits = "15")]
    pub gate_block: bool,

    /// Enable multi midstate processing = "AsicBoost"
    /// bit 31
    #[packed_field(bits = "7")]
    pub mmen: bool,
}

impl MiscCtrlReg {
    pub fn to_u32(&self) -> u32 {
        let reg_bytes = self.pack();

        let value = unsafe { mem::transmute(reg_bytes) };
        value
    }
}

/////
//impl<T> CtlCmd<T> {
//    fn new_for_fpga(cmd: Cmd, chip_address: u8, payload: T) -> FpgaCmd<Self, u32> {
//        let cmd_with_payload = Self {
//            header: CmdHeader::new(cmd, size_of::<T>(), chip_address, size_of::<u8>()),
//            payload,
//        };
//        FpgaCmd::new(cmd_with_payload)
//    }
//}
//impl<T> std::fmt::Debug for CtlCmd<T>
//where
//    T: std::fmt::Debug + std::marker::Copy,
//{
//    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
//        // we have to copy the payload as it may be unaligned
//        let header = self.header;
//        let payload = self.payload;
//
//        f.debug_struct("CtlCmd")
//            .field("header", &header)
//            .field("payload", &payload)
//            .finish()
//    }
//}
//impl<T> Copy for CtlCmd<T> where T: Copy { }

//impl<T> Clone for CtlCmd<T> {
//    fn clone(&self) -> Self {
//        Self {
//            header:
//
////            header: self.header,
////            payload: self.payload,
//        }
//    }
//}

#[cfg(test)]
mod test {
    use super::*;
    /// TODO: factor out command serialization tests into a macro
    /// Helper function for converting test data into fpga word slice
    fn u8_as_fpga_slice(cmd: &[u8]) -> &[u32] {
        unsafe {
            core::slice::from_raw_parts(cmd.as_ptr() as *const u32, cmd.len() / size_of::<u32>())
        }
    }

    #[test]
    //    /// Builds a sample 'Deactivate chain' command and verifies correct serialization
//    fn build_fpga_cmd() {
//        let assigned_cmd = [0x01u8, 0x02, 0x03, 0x04, 0x05];
//        let expected_cmd_with_padding = [0x55u8, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
//        let cmd = FpgaCmd::<_, u32>::new(assigned_cmd);
//        let expected_cmd_with_padding = u8_as_fpga_slice(&expected_cmd_with_padding);
//
//        assert_eq!(
//            cmd.as_slice(),
//            expected_cmd_with_padding,
//            "Incorrectly padded command:{:#04x?} sliced view: {:#010x?} expected view: \
//             {:#010x?}",
//            cmd,
//            cmd.as_slice(),
//            expected_cmd_with_padding
//        );
//    }

//    #[test]
//    /// Builds a sample 'Deactivate chain' command and verifies correct serialization
//    fn build_ctl_cmd() {
//        let cmd = Cmd {
//            code: 0x05,
//            to_all: true,
//            cmd_type: CmdType::VilCtlCmd,
//        };
//
//        let cmd = CtlCmd::new(cmd, 0, 0u16);
//        let expected_cmd_with_padding = [0x55u8, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
//        let expected_cmd_with_padding = u8_as_fpga_slice(&expected_cmd_with_padding);
//
//        assert_eq!(
//            cmd.as_slice(),
//            expected_cmd_with_padding,
//            "Incorrectly composed command:{:#04x?} sliced view: {:#010x?} expected view: \
//                 {:#010x?}",
//            cmd,
//            cmd.as_slice(),
//            expected_cmd_with_padding
//        );
//    }
    #[test]
    /// Builds a sample set_config command (here the PLL register @ 0x0c with a value of
    /// 0x21026800 that corresponds to
    /// and verifies correct serialization
    fn build_set_config_cmd() {
        let cmd = SetConfigCmd::new(0x00, false, PLL_PARAM_REG, 0x21026800);
        let expected_cmd_with_padding = [0x48u8, 0x09, 0x00, PLL_PARAM_REG, 0x00, 0x68, 0x02, 0x21];
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
        };
        let expected_reg = [0x13u8, 0x87, 0x90, 0x00];

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
        let broken_reg_bytes = [0x13u8, 0x86, 0x90, 0x04];
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
        let expected_reg = [0x40u8, 0x20, 0x9a, 0x80];

        let reg_bytes = reg.pack();

        assert_eq!(
            reg_bytes, expected_reg,
            "Incorrectly composed register:{:#04x?} sliced view: {:#04x?} expected view: \
             {:#04x?}",
            reg, reg_bytes, expected_reg
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
        let reg_value = reg.to_u32();
        assert_eq!(
            reg_value, expected_reg_value,
            "Misc Control Register 32-bit value  doesn't match: {} V:{:#010x} E:{:#010x}",
            reg, reg_value, expected_reg_value
        );
    }

}

//impl DeactivateChainCmd {
//    pub fn new() -> Self {
//        DeactivateChainCmd {
//            header: CmdHeader::new(Cmd(0), size_of::<u8>),
//        }
//    }
//}
