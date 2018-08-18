use std;
use std::mem::size_of;
use core;

bitfield!{
  /// Command
  pub struct Cmd(u8);
  impl Debug;
  code, set_code: 3, 0;
  to_all, set_to_all: 4;
  cmd_type, set_cmd_type: 7, 5;
}

/// Command types
enum CmdType {
    /// Control command for the chip
    VilCtlCmd = 0x02,
}

#[repr(C, packed)]
#[derive(Debug)]
/// Generic command header
struct CmdHeader {
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
            length: (size_of::<CmdHeader>() + payload_length + checksum_size) as u8,
            chip_address,
        }
    }
}

#[repr(C, packed)]
/// Represents a control command
/// T: arbitrary fixed length payload type
/// U:
struct CtlCmd<T, U> {
    header: CmdHeader,
    payload: T,
    /// Padding required by the FPGA, the exact type depends on the required format
    fpga_padding: U,
}

impl<T, U> CtlCmd<T, U> where
U: std::convert::From<u8> {
    fn new(cmd: Cmd, chip_address: u8, payload: T) -> Self {
        Self {
            header: CmdHeader::new(cmd, size_of::<T>(), chip_address, size_of::<u8>()),
            payload,
            fpga_padding: U::from(0),
        }
    }

    /// Provides slice view of the command that can be sent down the stream
    ///
    /// The slice consists of elements of U type. Typically the FPGA accepts data in multiples of
    /// U size. The slice contains necessary padding.
    ///
    /// TODO research whether there some standard trait that should be implemented instead of this
    /// method
    ///
    /// Example:
    /// ```rust
    /// let byte_view = self.as_slice()
    /// ```
    fn as_slice(&self) -> &[U] {
        let self_view = self as *const _ as *const U;
        // This produces command slice with necessary padding to be multiple of U and to prevent
        // truncation of any payload
        unsafe {
            core::slice::from_raw_parts(self_view, size_of::<Self>()/size_of::<U>())
        }
    }
}
/// Custom debug due to the fact that we are printing out a packed structure. Specifically, it is
/// forbidden to borrow potentially unaligned fields (see https://github
/// .com/rust-lang/rust/issues/46043)
impl<T, U> std::fmt::Debug for CtlCmd<T, U>
where T: std::fmt::Debug + std::marker::Copy {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // we have to copy the payload as it may be unaligned
        let payload = self.payload;

        f.debug_struct("CtlCmd").field("header", &self.header)
            .field("payload", &payload).finish()
    }

}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    /// Builds a sample 'Deactivate chain' command and verifies correct serialization
    fn build_ctl_cmd() {
        let mut cmd = Cmd(0);
        cmd.set_cmd_type(CmdType::VilCtlCmd as u8);
        cmd.set_code(0x05);
        cmd.set_to_all(true);

        let cmd = CtlCmd::<_, u32>::new(cmd,0,0u16);
        let expected_cmd_with_padding = [0x55u8, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        let expected_cmd_with_padding: &[u32] = unsafe {
            core::slice::from_raw_parts(expected_cmd_with_padding.as_ptr() as *const u32,
                                        expected_cmd_with_padding.len() / size_of::<u32>())
        };
        assert_eq!(cmd.as_slice(), expected_cmd_with_padding,
                   "Incorrectly composed command:{:#04x?} sliced view: {:#010x?} expected view: \
                   {:#010x?}",
                   cmd, cmd.as_slice(), expected_cmd_with_padding);
    }
}

//impl DeactivateChainCmd {
//    pub fn new() -> Self {
//        DeactivateChainCmd {
//            header: CmdHeader::new(Cmd(0), size_of::<u8>),
//        }
//    }
//}
