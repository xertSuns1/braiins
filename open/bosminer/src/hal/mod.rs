use std::io;
use uint;

pub mod s9;

/// Describes actual mining work for submission to a hashing hardware
/// # TODO
/// Add ntime limit for supporting hardware that can do nTime rolling on its own
pub struct MiningWork {
    /// Version field used for calculating the midstate
    pub version: u32,
    /// Extranonce 2 used for calculating merkelroot
    pub extranonce_2: u32,
    /// Multiple midstates can be generated for each work - these are the full
    pub midstates: Vec<uint::U256>,
    /// least-significant word of merkleroot that goes to chunk2 of SHA256
    pub merkel_root_lsw: u32,
    /// Start value for nTime, hardware may roll nTime further.
    pub ntime: u32,
    /// Network difficulty encoded as nbits (exponent + mantissa - see
    /// https://en.bitcoin.it/wiki/Difficulty)
    pub nbits: u32,
}

/// Represents raw result from the mining hardware
#[derive(Debug)]
pub struct MiningWorkResult {
    /// actual nonce
    pub nonce: u32,
    /// nTime of the result in case the HW also rolls the nTime field
    pub ntime: Option<u32>,
    /// index of a midstate corresponds to the found nonce
    pub midstate_idx: usize,
    /// Unique identifier for the result
    pub result_id: u32,
}

/// Any hardware mining controller should implement at least these methods
pub trait HardwareCtl {
    /// Sends work to the hash chain
    ///
    /// Returns a unique ID that can be used for registering the work within a hardware specific
    /// registry
    fn send_work(&mut self, work: &MiningWork) -> Result<u32, io::Error>;

    /// Receives 1 MiningWorkResult
    fn recv_work_result(&mut self) -> Result<Option<MiningWorkResult>, io::Error>;
}
