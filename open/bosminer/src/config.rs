/// This module holds bOSminer configuration until better solution comes around.

/// We re-export individually each config option to avoid creating
/// multiple independent (global) configuration schemas. Having a per-platform
/// (local) option is OK but it shouldn't leak outside platform module.
///
/// If you get UNRESOLVED IMPORT here, it probably means someone added
/// configuration option for one architecture but not for all of them.
pub use crate::hal::config::{ASIC_DIFFICULTY, DEFAULT_MIDSTATE_COUNT, JOB_TIMEOUT};
