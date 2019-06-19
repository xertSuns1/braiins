use crate::hal;
use crate::workhub;

use bitcoin_hashes::sha256d::Hash;
use stratum::v2::messages;

#[derive(Copy, Clone)]
struct StratumJob {
    hash: Hash,
}

impl hal::BitcoinJob for StratumJob {
    fn version(&self) -> u32 {
        0
    }

    fn version_mask(&self) -> u32 {
        0
    }

    fn previous_hash(&self) -> &Hash {
        &self.hash
    }

    fn merkle_root(&self) -> &Hash {
        &self.hash
    }

    fn time(&self) -> u32 {
        0xffff_ffff
    }

    fn bits(&self) -> u32 {
        0xffff_ffff
    }
}

struct StratumClient;

impl StratumClient {}
