use uint;

pub mod s9;

/// Describes actual mining work for submission to a hashing hardware.
/// Starting with merkel_root_lsw the data goes to chunk2 of SHA256.
///
/// NOTE: eventhough, version and extranonce_2 are already included in the midstates, we
/// need them as part of the MiningWork structure. The reason is stratum submission requirements.
/// This may need further refactoring.
/// # TODO
/// Add ntime limit for supporting hardware that can do nTime rolling on its own
#[derive(Clone, Debug)]
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

/// Represents raw solution from the mining hardware
#[derive(Clone, Debug)]
pub struct MiningWorkSolution {
    /// actual nonce
    pub nonce: u32,
    /// nTime of the solution in case the HW also rolls the nTime field
    pub ntime: Option<u32>,
    /// index of a midstate that corresponds to the found nonce
    pub midstate_idx: usize,
    /// Unique identifier for the solution
    pub solution_id: u32,
}

/// Any hardware mining controller should implement at least these methods
pub trait HardwareCtl {
    /// Sends work to the hash chain
    ///
    /// Returns a unique ID that can be used for registering the work within a hardware specific
    /// registry
    fn send_work(&mut self, work: &MiningWork) -> Result<u32, failure::Error>;

    /// Receives 1 MiningWorkSolution
    fn recv_solution(&mut self) -> Result<Option<MiningWorkSolution>, failure::Error>;

    /// Extracts original work ID for a mining solution
    fn get_work_id_from_solution(&self, solution: &MiningWorkSolution) -> u32;

    /// Returns the number of detected chips
    fn get_chip_count(&self) -> usize;
}
