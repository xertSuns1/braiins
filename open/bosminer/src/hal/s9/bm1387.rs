use core;
use packed_struct::prelude::*;
use std::mem::size_of;

#[derive(PackedStruct, Debug)]
#[packed_struct(size_bytes = "1", bit_numbering = "lsb0")]
/// Command
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

#[derive(PrimitiveEnum_u8, Clone, Copy, Debug, PartialEq)]
/// Command types
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
    /// * `checksum_size` - Size of checksum needs to be known as it is accounted in the length
    /// field
    fn new(cmd: Cmd, payload_length: usize, chip_address: u8, checksum_size: usize) -> Self {
        CmdHeader {
            cmd,
            length: (Self::packed_bytes() + payload_length + checksum_size) as u8,
            chip_address,
        }
    }
    fn new_ctl_cmd_header(cmd: Cmd, payload_length: usize, chip_address: u8) -> Self {
        Self::new(cmd, payload_length, chip_address, size_of::<u8>())
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
    header: CmdHeader,
    register: u8,
    value: u32,
}
impl SetConfigCmd {
    fn new(chip_address: u8, to_all: bool, register: u8, value: u32) -> SetConfigCmd {
        let cmd = Cmd::new(0x08, to_all);
        // payload consists of 1 byte register address and 4 byte value
        let header =
            CmdHeader::new_ctl_cmd_header(cmd, size_of::<u8>() + size_of::<u32>(), chip_address);
        Self {
            header,
            register,
            value,
        }
    }
}

struct GetConfigCmd {
    header: CmdHeader,
    register: u8,
}
impl GetConfigCmd {
    fn new(chip_address: u8, to_all: bool, register: u8) -> GetConfigCmd {
        let cmd = Cmd::new(0x04, to_all);
        let header = CmdHeader::new_ctl_cmd_header(cmd, size_of::<u8>(), chip_address);
        Self { header, register }
    }
}

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
        let cmd = SetConfigCmd::new(0x00, false, 0x0c, 0x21026800);
        let expected_cmd_with_padding = [0x48u8, 0x09, 0x00, 0x0c, 0x00, 0x68, 0x02, 0x21];
        //        let expected_cmd_with_padding = u8_as_fpga_slice(&expected_cmd_with_padding);
        let cmd_bytes = cmd.pack();
        assert_eq!(
            cmd_bytes, expected_cmd_with_padding,
            "Incorrectly composed command:{:#04x?} sliced view: {:#010x?} expected view: \
             {:#010x?}",
            cmd, cmd_bytes, expected_cmd_with_padding
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
