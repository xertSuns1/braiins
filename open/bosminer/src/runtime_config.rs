/// This module manages bosminer runtime configuration
/// It will be refactored into something more flexible once we decide on how to manage the configuration.
use crate::config;
use lazy_static::lazy_static;
use std::sync::Mutex;

/// Structure representing miner configuration
pub struct RunTimeConfig {
    pub midstate_count: usize,
}

impl RunTimeConfig {
    pub fn new() -> Self {
        Self {
            midstate_count: config::DEFAULT_MIDSTATE_COUNT,
        }
    }
}

lazy_static! {
    /// Shared (global) configuration structure
    pub static ref CONFIG: Mutex<RunTimeConfig> = Mutex::new(RunTimeConfig::new());
}

/// This function is only temporary, until we unify midstate_count passing across s9 backend
pub fn get_midstate_count() -> usize {
    CONFIG.lock().expect("config lock").midstate_count
}
