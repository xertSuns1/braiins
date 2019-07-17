use crate::btc;

use packed_struct::prelude::*;
use packed_struct_codegen::PackedStruct;

use std::mem::size_of;
use std::time::Duration;

pub const ID_VENDOR: u16 = 0x10c4;
pub const ID_PRODUCT: u16 = 0xea60;

pub const DEVICE_IFACE: u8 = 0;
pub const DEVICE_CONFIGURATION: u8 = 1;

pub const WRITE_ADDR: u8 = 0x1;
pub const READ_ADDR: u8 = 0x81;

pub const WAIT_TIMEOUT: Duration = Duration::from_millis(100);

pub const WORK_PAYLOAD_SIZE: usize = 64;

/// Icarus work payload containing all information for finding Bitcoin block header nonce
#[derive(PackedStruct, Debug, Clone, Copy, Default)]
#[packed_struct(endian = "lsb")]
pub struct WorkPayload {
    midstate: [u32; 8],
    check: u8,
    data: u8,
    cmd: u8,
    prefix: u8,
    unused: [u8; 15],
    id: u8,
    pub bits: u32,
    pub time: u32,
    pub merkle_tail: u32,
}

impl WorkPayload {
    pub fn new(midstate: &btc::Midstate, merkle_tail: u32, time: u32, bits: u32) -> Self {
        // midstate 32bit words are stored in array in a reverse order
        let mut midstate_words = [0u32; btc::SHA256_DIGEST_SIZE / size_of::<u32>()];
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
    use crate::hal::BitcoinJob;
    use crate::test_utils;

    #[test]
    fn test_work_payload() {
        for block in test_utils::TEST_BLOCKS.iter() {
            let work_header = WorkPayload::new(
                &block.midstate,
                block.merkle_root_tail(),
                block.time(),
                block.bits(),
            );

            // check binary representation of Icarus work header
            assert_eq!(&block.icarus_bytes[..], &work_header.into_bytes()[..]);
        }
    }
}
