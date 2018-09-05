use uint;

pub mod s9;

/// Describes actual mining work for submission to a hashing hardware
pub struct MiningWork<'a> {
    /// Version field used for calculating the midstate
    version: u32,
    /// Extranonce 2 used for calculating merkelroot
    extranonce_2: u32,
    /// array
    midstates: &'a [uint::U256],
    merkel_root_lsw: u32,
    ntime: u32,
    nbits: u32,
}

/// Represents raw result from the mining hardware
pub struct MiningResult<'a> {
    /// actual nonce
    nonce: u32,
    /// nTime of the result in case the HW also rolls the nTime field
    ntime: u32,
    /// index of a midstate corresponds to the found nonce
    midstate_idx: usize,
    orig_work: &'a MiningWork<'a>,
}

/// Any hardware mining controller should implement at least these methods
pub trait HardwareCtl {
    fn send_work(&self, work: &MiningWork);
}
