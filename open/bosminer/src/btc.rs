use packed_struct::prelude::*;
use packed_struct_codegen::PackedStruct;

use bitcoin_hashes::{sha256d, sha256d::Hash, Hash as HashTrait};

/// SHA256 digest size used in Bitcoin protocol
pub const SHA256_DIGEST_SIZE: usize = 32;

/// A Bitcoin block header is 80 bytes long
pub const BLOCK_HEADER_SIZE: usize = 80;

/// Bitcoin block header structure which can be packed to binary representation
/// which is 80 bytes long
#[derive(PackedStruct, Debug, Clone, Copy)]
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
}
