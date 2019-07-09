use crate::hal;

use std::sync::Arc;

use bitcoin_hashes::{sha256, Hash, HashEngine};
use byteorder::{ByteOrder, LittleEndian};

// TODO: move to BTC
const VERSION_MASK: u32 = 0x1fffe000;
const VERSION_SHIFT: u32 = 13;

pub struct VersionRolling {
    job: Arc<dyn hal::BitcoinJob>,
    /// Number of midstates that each generated work covers
    midstates: u16,
    /// Starting value of the rolled part of the version (before BIP320 shift)
    curr_version: u16,
    /// Base Bitcoin block header version with BIP320 bits cleared
    base_version: u32,
}

impl VersionRolling {
    pub fn new(job: Arc<dyn hal::BitcoinJob>, midstates: u16) -> Self {
        let base_version = job.version() & !VERSION_MASK;
        Self {
            job,
            midstates,
            curr_version: 0,
            base_version,
        }
    }

    fn get_block_version(&self, version: u16) -> u32 {
        self.base_version | ((version as u32) << VERSION_SHIFT)
    }

    /// Roll new versions for the block header for all midstates
    /// Return None If the rolled version space is exhausted. The version range can be
    /// reset by specifying `new_job`
    fn next_versions(&mut self) -> Vec<u32> {
        // Allocate the range for all midstates as per the BIP320 rolled 16 bits
        let version_start = self.curr_version;
        if let Some(next_version) = self.curr_version.checked_add(self.midstates) {
            self.curr_version = next_version;
        } else {
            return vec![];
        }

        // Convert the allocated range to a list of versions as per BIP320
        let mut versions = Vec::with_capacity(self.midstates as usize);
        for version in version_start..self.curr_version {
            versions.push(self.get_block_version(version));
        }
        versions
    }
}

impl hal::WorkEngine for VersionRolling {
    fn next_work(&mut self) -> Option<hal::MiningWork> {
        let versions = self.next_versions();
        if versions.is_empty() {
            return None;
        }

        let mut midstates = Vec::with_capacity(versions.len());

        let mut engine = sha256::Hash::engine();
        let buffer = &mut [0u8; 64];

        buffer[4..36].copy_from_slice(&self.job.previous_hash().into_inner());
        buffer[36..64].copy_from_slice(&self.job.merkle_root().into_inner()[..32 - 4]);

        for version in versions {
            LittleEndian::write_u32(&mut buffer[0..4], version);
            engine.input(buffer);
            midstates.push(hal::Midstate {
                version,
                state: engine.midstate(),
            })
        }

        Some(hal::MiningWork {
            job: self.job.clone(),
            midstates,
            ntime: self.job.time(),
        })
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::hal::WorkEngine;
    use crate::test_utils;

    #[test]
    fn test_block_midstate() {
        for block in test_utils::TEST_BLOCKS.iter() {
            let job = Arc::new(*block);
            let mut engine = VersionRolling::new(job, 1);

            let work = engine.next_work().unwrap();
            assert_eq!(block.midstate, work.midstates[0].state);
        }
    }
}
