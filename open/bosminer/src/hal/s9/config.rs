/// This module holds configuration for S9 miner until better solution (registry of sorts?) is implemented.
use std::time::Duration;

/// Default number of midstates
pub const DEFAULT_MIDSTATE_COUNT: usize = 4;

/// Index of hashboard that is to be instantiated
pub const S9_HASHBOARD_INDEX: usize = 8;

/// Default ASIC difficulty
pub const ASIC_DIFFICULTY: usize = 256;

/// Maximum time it takes to compute one job under normal circumstances
pub const JOB_TIMEOUT: Duration = Duration::from_secs(5);
