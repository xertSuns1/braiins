use std::time::Duration;

/// Number of midstates
pub const DEFAULT_MIDSTATE_COUNT: usize = 1;

/// Default ASIC difficulty
pub const ASIC_DIFFICULTY: usize = 1;

/// Maximum time it takes to compute one job under normal circumstances
pub const JOB_TIMEOUT: Duration = Duration::from_secs(30);
