use packed_struct::prelude::*;
use packed_struct_codegen::PackedStruct;

use bitcoin_hashes::{sha256, sha256d, sha256d::Hash, Hash as HashTrait, HashEngine};

/// SHA256 digest size used in Bitcoin protocol
pub const SHA256_DIGEST_SIZE: usize = 32;

/// Binary representation of target for difficulty 1
pub const DIFFICULTY_1_TARGET_BYTES: [u8; SHA256_DIGEST_SIZE] = [
    0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// A Bitcoin block header is 80 bytes long
pub const BLOCK_HEADER_SIZE: usize = 80;

/// First chunk of Bitcoin block header used for midstate computation
pub const BLOCK_HEADER_CHUNK1_SIZE: usize = 64;

/// Bitcoin block header structure which can be packed to binary representation
/// which is 80 bytes long
#[derive(PackedStruct, Debug, Clone, Copy, Default)]
#[packed_struct(endian = "lsb")]
pub struct BlockHeader {
    /// Version field that reflects the current network consensus and rolled bits
    pub version: u32,
    /// Double SHA256 hash of the previous block header
    pub previous_hash: [u8; 32],
    /// Double SHA256 hash based on all of the transactions in the block
    pub merkle_root: [u8; 32],
    /// Current block timestamp as seconds since 1970-01-01T00:00 UTC
    pub time: u32,
    /// Current target in compact format (network difficulty)
    pub bits: u32,
    /// The nonce used to generate this block witch is bellow pool/network target
    pub nonce: u32,
}

impl BlockHeader {
    /// Get binary representation of Bitcoin block header
    #[inline]
    pub fn into_bytes(self) -> [u8; BLOCK_HEADER_SIZE] {
        self.pack()
    }

    /// Compute SHA256 double hash
    pub fn hash(&self) -> Hash {
        let block_bytes = self.into_bytes();
        sha256d::Hash::hash(&block_bytes[..])
    }

    /// Compute SHA256 midstate from first chunk of block header
    pub fn midstate(&self) -> Midstate {
        let mut engine = sha256::Hash::engine();
        engine.input(&self.into_bytes()[..BLOCK_HEADER_CHUNK1_SIZE]);
        engine.midstate().into()
    }
}

/// Array containing SHA256 digest
type Sha256Array = [u8; SHA256_DIGEST_SIZE];

/// Type representing SHA256 midstate used for conversion simplification and printing
#[derive(Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct Midstate(Sha256Array);

impl Midstate {
    pub fn as_slice(&self) -> &[u8] {
        &self.0[..]
    }

    pub fn from_hex(s: &str) -> Result<Self, bitcoin_hashes::Error> {
        Ok(Self(bitcoin_hashes::hex::FromHex::from_hex(s)?))
    }
}

impl From<Sha256Array> for Midstate {
    /// Get midstate from binary representation of SHA256
    fn from(bytes: Sha256Array) -> Self {
        Self(bytes)
    }
}

impl AsRef<Sha256Array> for Midstate {
    fn as_ref(&self) -> &Sha256Array {
        &self.0
    }
}

macro_rules! hex_fmt_impl(
    ($imp:ident, $ty:ident) => (
        impl ::std::fmt::$imp for $ty {
            fn fmt(&self, fmt: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                ::bitcoin_hashes::hex::format_hex(self.as_slice(), fmt)
            }
        }
    )
);

hex_fmt_impl!(Debug, Midstate);
hex_fmt_impl!(Display, Midstate);
hex_fmt_impl!(LowerHex, Midstate);

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::hal::BitcoinJob;
    use crate::test_utils;

    use bitcoin_hashes::hex::ToHex;

    #[test]
    fn test_block_header() {
        for block in test_utils::TEST_BLOCKS.iter() {
            let block_header = BlockHeader {
                version: block.version(),
                previous_hash: block.previous_hash().into_inner(),
                merkle_root: block.merkle_root().into_inner(),
                time: block.time(),
                bits: block.bits(),
                nonce: block.nonce,
            };

            // test computation of SHA256 double hash of Bitcoin block header
            let block_hash = block_header.hash();
            assert_eq!(block.hash, block_hash);

            // check expected format of hash in hex string with multiple formaters
            assert_eq!(block.hash_str, block_hash.to_hex());
            assert_eq!(block.hash_str, format!("{}", block_hash));
            assert_eq!(block.hash_str, format!("{:?}", block_hash));
            assert_eq!(block.hash_str, format!("{:x}", block_hash));

            // check binary representation of Bitcoin block header
            assert_eq!(&block.header_bytes[..], &block_header.into_bytes()[..]);
        }
    }

    #[test]
    fn test_block_header_midstate() {
        for block in test_utils::TEST_BLOCKS.iter() {
            let block_header = BlockHeader {
                version: block.version(),
                previous_hash: block.previous_hash().into_inner(),
                merkle_root: block.merkle_root().into_inner(),
                ..Default::default()
            };

            // test computation of SHA256 midstate of Bitcoin block header
            let block_midstate = block_header.midstate();
            assert_eq!(block.midstate, block_midstate);

            // check expected format of midstate in hex string with multiple formaters
            assert_eq!(block.midstate_str, block_midstate.to_hex());
            assert_eq!(block.midstate_str, format!("{}", block_midstate));
            assert_eq!(block.midstate_str, format!("{:?}", block_midstate));
            assert_eq!(block.midstate_str, format!("{:x}", block_midstate));
        }
    }
}
