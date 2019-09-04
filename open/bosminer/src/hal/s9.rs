mod bm1387;
pub mod config;
pub mod error;
pub mod fifo;
pub mod gpio;
pub mod null_work;
pub mod power;
pub mod registry;
#[cfg(test)]
pub mod test;

use ii_logging::macros::*;

use crate::hal::{self, s9};
use crate::runtime_config;

// TODO: remove thread specific components
use std::sync::Arc;
use std::thread;

use lazy_static::lazy_static;

use std::time::{Duration, SystemTime};

use tokio::await;

use error::ErrorKind;
use failure::ResultExt;

use crate::work;

use futures_locks::Mutex;

use byteorder::{ByteOrder, LittleEndian};
use packed_struct::{PackedStruct, PackedStructSlice};

use embedded_hal::digital::v2::InputPin;
use embedded_hal::digital::v2::OutputPin;

use ii_fpga_io_am1_s9::hchainio0::ctrl_reg::MIDSTATE_CNT_A;

/// Timing constants
const INACTIVATE_FROM_CHAIN_DELAY_MS: u64 = 100;
/// Base delay quantum during hashboard initialization
const INIT_DELAY_MS: u64 = 1000;

/// Maximum number of chips is limitted by the fact that there is only 8-bit address field and
/// addresses to the chips need to be assigned with step of 4 (e.g. 0, 4, 8, etc.)
const MAX_CHIPS_ON_CHAIN: usize = 64;

/// Oscillator speed for all chips on S9 hash boards
const CHIP_OSC_CLK_HZ: usize = 25_000_000;

/// Exact value of the initial baud rate after reset of the hashing chips.
const INIT_CHIP_BAUD_RATE: usize = 115740;
/// Exact desired target baud rate when hashing at full speed (matches the divisor, too)
const TARGET_CHIP_BAUD_RATE: usize = 1562500;

/// Base clock speed of the IP core running in the FPGA
const FPGA_IPCORE_F_CLK_SPEED_HZ: usize = 50_000_000;
/// Divisor of the base clock. The resulting clock is connected to UART
const FPGA_IPCORE_F_CLK_BASE_BAUD_DIV: usize = 16;

/// Default PLL frequency for clocking the chips
const DEFAULT_S9_PLL_FREQUENCY: u64 = 650_000_000;

/// Default initial voltage
const INITIAL_VOLTAGE: power::Voltage = power::Voltage::from_volts(9.4);

/// Default PLL value (650 MHz)
const DEFAULT_PLL_CONFIG: u32 = 0x21026800;

lazy_static! {
    /// What is our target?
    static ref ASIC_TARGET: ii_bitcoin::Target =
        ii_bitcoin::Target::from_pool_difficulty(config::ASIC_DIFFICULTY);
}

/// `MidstateCount` represents the number of midstates S9 FPGA sends to chips.
/// This information needs to be accessible to everyone that processes `work_id`.
///
/// `MidstateCount` provides methods to encode number of midstates in various ways:
///  * bitmask to mask out parts of `solution_id`
///  * base-2 logarithm of number of midstates
///  * FPGA configuration value (which is base-2 logarithm, but as an enum)
///
/// `MidstateCount` is always valid - creation of `MidstateCount` object that isn't
/// supported by hardware shouldn't be possible.
#[derive(Debug, Clone, Copy)]
pub struct MidstateCount {
    /// internal representation is base-2 logarithm of number of midstates
    log2: usize,
}

impl MidstateCount {
    /// Construct Self, panic if number of midstates is not valid for this hw
    fn new(count: usize) -> Self {
        match count {
            1 => Self { log2: 0 },
            2 => Self { log2: 1 },
            4 => Self { log2: 2 },
            _ => panic!("Unsupported S9 Midstate count {}", count),
        }
    }

    /// Return midstate count encoded for FPGA
    fn to_reg(&self) -> MIDSTATE_CNT_A {
        match self.log2 {
            0 => MIDSTATE_CNT_A::ONE,
            1 => MIDSTATE_CNT_A::TWO,
            2 => MIDSTATE_CNT_A::FOUR,
            _ => panic!("invalid internal midstate count logarithm"),
        }
    }

    /// Return midstate count
    #[inline]
    fn to_count(&self) -> usize {
        1 << self.log2
    }

    /// Return midstate count mask (to get midstate_idx bits from `work_id`)
    #[inline]
    fn to_mask(&self) -> usize {
        (1 << self.log2) - 1
    }
}

/// `SolutionId` provides parsing and representation of "solution id" part of
/// chip response read as second word from FPGA FIFO.
#[derive(Debug, Clone)]
struct SolutionId {
    pub work_id: usize,
    pub midstate_idx: usize,
    pub solution_idx: usize,
}

impl SolutionId {
    /// Bit position where work ID starts in the second word provided by the IP core with mining work
    /// solution
    const WORK_ID_OFFSET: usize = 8;

    /// Extract fields of "solution id" word into Self
    pub fn from_reg(solution_reg: u32, midstate_count: MidstateCount) -> Self {
        let solution_id = (solution_reg & 0xffffff) as usize;
        let work_id_ext = solution_id >> Self::WORK_ID_OFFSET;
        Self {
            solution_idx: solution_id & ((1 << Self::WORK_ID_OFFSET) - 1),
            work_id: work_id_ext & !midstate_count.to_mask(),
            midstate_idx: work_id_ext & midstate_count.to_mask(),
        }
    }
}

/// `WorkIdGen` represents a wrapping 16-bit counter used for `work_id` generation.
/// The counter increments by number of midstates.
#[derive(Debug, Clone)]
struct WorkIdGen {
    midstate_count: MidstateCount,
    work_id: u16,
}

impl WorkIdGen {
    pub fn new(midstate_count: MidstateCount) -> Self {
        Self {
            midstate_count,
            work_id: 0,
        }
    }

    pub fn next(&mut self) -> usize {
        let retval = self.work_id as usize;
        // compiler has to know that work ID rolls over regularly
        self.work_id = self
            .work_id
            .wrapping_add(self.midstate_count.to_count() as u16);
        retval
    }
}

/// Hash Chain Controller provides abstraction of the FPGA interface for operating hashing boards.
/// It is the user-space driver for the IP Core
///
/// Main responsibilities:
/// - memory mapping of the FPGA control interface
/// - mining work submission and solution processing
///
/// TODO: implement drop trait (results in unmap)
/// TODO: rename to HashBoardCtrl and get rid of the hash_chain identifiers + array
pub struct HChainCtl<VBackend> {
    /// Current work ID once it rolls over, we can start retiring old jobs
    work_id_gen: WorkIdGen,
    /// Number of chips that have been detected
    chip_count: usize,
    /// Eliminates the need to query the IP core about the current number of configured midstates
    midstate_count: MidstateCount,
    /// ASIC difficulty
    asic_difficulty: usize,
    /// PLL frequency
    pll_frequency: u64,
    /// Voltage controller on this hashboard
    /// TODO: consider making voltage ctrl a shared instance so that heartbeat and regular
    /// processing can use it. More: the backend should also become shared instance?
    voltage_ctrl: power::VoltageCtrl<VBackend>,
    /// Plug pin that indicates the hashboard is present
    #[allow(dead_code)]
    plug_pin: gpio::PinIn,
    /// Pin for resetting the hashboard
    rst_pin: gpio::PinOut,
    /// When the heartbeat was last sent
    #[allow(dead_code)]
    last_heartbeat_sent: Option<SystemTime>,
    #[allow(dead_code)]
    hashboard_idx: usize,
    pub cmd_fifo: fifo::HChainFifo,
    pub work_rx_fifo: Option<fifo::HChainFifo>,
    pub work_tx_fifo: Option<fifo::HChainFifo>,
}

unsafe impl<VBackend> Send for HChainCtl<VBackend> {}
unsafe impl<VBackend> Sync for HChainCtl<VBackend> {}

impl<VBackend> HChainCtl<VBackend>
where
    VBackend: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    /// Creates a new hashboard controller with memory mapped FPGA IP core
    ///
    /// * `gpio_mgr` - gpio manager used for producing pins required for hashboard control
    /// * `voltage_ctrl_backend` - communication backend for the voltage controller
    /// * `hashboard_idx` - index of this hashboard determines which FPGA IP core is to be mapped
    /// * `midstate_count` - see Self
    /// TODO: asic_difficulty
    pub fn new(
        gpio_mgr: &gpio::ControlPinManager,
        voltage_ctrl_backend: VBackend,
        hashboard_idx: usize,
        midstate_count: MidstateCount,
        asic_difficulty: usize,
    ) -> error::Result<Self> {
        // Hashboard creation is aborted if the pin is not present
        let plug_pin = gpio_mgr
            .get_pin_in(gpio::PinInName::Plug(hashboard_idx))
            .context(ErrorKind::Hashboard(
                hashboard_idx,
                "failed to initialize plug pin".to_string(),
            ))?;
        // also detect that the board is present
        if plug_pin.is_low()? {
            Err(ErrorKind::Hashboard(
                hashboard_idx,
                "not present".to_string(),
            ))?
        }

        // Instantiate the reset pin
        let rst_pin = gpio_mgr
            .get_pin_out(gpio::PinOutName::Rst(hashboard_idx))
            .context(ErrorKind::Hashboard(
                hashboard_idx,
                "failed to initialize reset pin".to_string(),
            ))?;

        let mut cmd_fifo = fifo::HChainFifo::new(hashboard_idx, midstate_count)?;
        let mut work_rx_fifo = fifo::HChainFifo::new(hashboard_idx, midstate_count)?;
        let mut work_tx_fifo = fifo::HChainFifo::new(hashboard_idx, midstate_count)?;

        cmd_fifo.init()?;
        work_rx_fifo.init()?;
        work_tx_fifo.init()?;

        Ok(Self {
            work_id_gen: WorkIdGen::new(midstate_count),
            chip_count: 0,
            midstate_count,
            asic_difficulty,
            voltage_ctrl: power::VoltageCtrl::new(voltage_ctrl_backend, hashboard_idx),
            plug_pin,
            rst_pin,
            hashboard_idx,
            last_heartbeat_sent: None,
            // TODO: implement setting me
            pll_frequency: DEFAULT_S9_PLL_FREQUENCY,
            cmd_fifo: cmd_fifo,
            work_rx_fifo: Some(work_rx_fifo),
            work_tx_fifo: Some(work_tx_fifo),
        })
    }
    /// Calculate work_time for this instance of HChain
    ///
    /// Returns number of ticks (suitable to be written to `WORK_TIME` register)
    #[inline]
    fn calculate_work_time(&self) -> u32 {
        secs_to_fpga_ticks(calculate_work_delay_for_pll(
            self.midstate_count.to_count() as u64,
            self.pll_frequency,
        ))
    }

    /// Helper method that initializes the FPGA IP core
    fn ip_core_init(&mut self) -> error::Result<()> {
        // Disable ip core
        self.cmd_fifo.disable_ip_core();
        self.cmd_fifo.enable_ip_core();

        self.set_ip_core_baud_rate(INIT_CHIP_BAUD_RATE)?;
        let work_time = self.calculate_work_time();
        trace!("Using work time: {}", work_time);
        self.cmd_fifo.set_ip_core_work_time(work_time);
        self.cmd_fifo
            .set_ip_core_midstate_count(self.midstate_count.to_reg());

        Ok(())
    }

    /// Puts the board into reset mode and disables the associated IP core
    fn enter_reset(&mut self) -> error::Result<()> {
        self.cmd_fifo.disable_ip_core();
        // perform reset of the hashboard
        self.rst_pin.set_low()?;
        Ok(())
    }

    /// Leaves reset mode
    fn exit_reset(&mut self) -> error::Result<()> {
        self.rst_pin.set_high()?;
        self.cmd_fifo.enable_ip_core();
        Ok(())
    }

    /// Configures difficulty globally on all chips within the hashchain
    fn set_asic_diff(&self, difficulty: usize) -> error::Result<()> {
        let tm_reg = bm1387::TicketMaskReg::new(difficulty as u32)?;
        trace!(
            "Setting ticket mask register for difficulty {}, value {:#010x?}",
            difficulty,
            tm_reg
        );
        let cmd = bm1387::SetConfigCmd::new(0, true, bm1387::TICKET_MASK_REG, tm_reg.into());
        // wait until all commands have been sent
        self.send_ctl_cmd(&cmd.pack(), true);

        // Verify we were able to set the difficulty on all chips correctly
        let get_tm_cmd = bm1387::GetStatusCmd::new(0, true, bm1387::TICKET_MASK_REG).pack();
        self.send_ctl_cmd(&get_tm_cmd, true);
        // TODO: verify reply equals to value we set
        // TODO: implement async mechanism to send/wait for commands
        Ok(())
    }

    /// Initializes the complete hashboard including enumerating all chips
    pub fn init(&mut self) -> error::Result<()> {
        self.ip_core_init()?;
        info!("Hashboard IP core initialized");
        self.voltage_ctrl.reset()?;
        info!("Voltage controller reset");
        self.voltage_ctrl.jump_from_loader_to_app()?;
        info!("Voltage controller application started");
        let version = self.voltage_ctrl.get_version()?;
        info!("Voltage controller firmware version {:#04x}", version);
        // TODO accept multiple
        if version != power::EXPECTED_VOLTAGE_CTRL_VERSION {
            // TODO: error!("{}", err_msg);
            Err(ErrorKind::UnexpectedVersion(
                "voltage controller firmware".to_string(),
                version.to_string(),
                power::EXPECTED_VOLTAGE_CTRL_VERSION.to_string(),
            ))?
        }
        // Voltage controller successfully initialized at this point, we should start sending
        // heart beats to it. Otherwise, it would shut down in about 10 seconds.
        info!("Starting voltage controller heart beat task");
        let _ = self.voltage_ctrl.start_heart_beat_task();

        self.voltage_ctrl.set_voltage(INITIAL_VOLTAGE)?;
        self.voltage_ctrl.enable_voltage()?;
        info!("Resetting hash board");
        self.enter_reset()?;
        // disable voltage
        self.voltage_ctrl.disable_voltage()?;
        thread::sleep(Duration::from_millis(INIT_DELAY_MS));
        self.voltage_ctrl.enable_voltage()?;
        thread::sleep(Duration::from_millis(2 * INIT_DELAY_MS));

        // TODO consider including a delay
        self.exit_reset()?;
        thread::sleep(Duration::from_millis(INIT_DELAY_MS));
        //        let voltage = self.voltage_ctrl.get_voltage()?;
        //        if voltage != 0 {
        //            return Err(io::Error::new(
        //                io::ErrorKind::Other, format!("Detected voltage {}", voltage)));
        //        }
        info!("Starting chip enumeration");
        self.enumerate_chips()?;
        info!("Discovered {} chips", self.chip_count);

        // set PLL
        self.set_pll()?;

        // configure the hashing chain to operate at desired baud rate. Note that gate block is
        // enabled to allow continuous start of chips in the chain
        self.configure_hash_chain(TARGET_CHIP_BAUD_RATE, false, true)?;
        self.set_ip_core_baud_rate(TARGET_CHIP_BAUD_RATE)?;

        self.set_asic_diff(self.asic_difficulty)?;
        Ok(())
    }

    /// Detects the number of chips on the hashing chain and assigns an address to each chip
    fn enumerate_chips(&mut self) -> error::Result<()> {
        // Enumerate all chips (broadcast read address register request)
        let get_addr_cmd = bm1387::GetStatusCmd::new(0, true, bm1387::GET_ADDRESS_REG).pack();
        self.send_ctl_cmd(&get_addr_cmd, true);
        self.chip_count = 0;
        while let Some(addr_reg) = self.recv_ctl_cmd_resp::<bm1387::GetAddressReg>()? {
            if addr_reg.chip_rev != bm1387::ChipRev::Bm1387 {
                Err(ErrorKind::Hashchip(format!(
                    "unexpected revision of chip {} (expected: {:?} received: {:?})",
                    self.chip_count,
                    addr_reg.chip_rev,
                    bm1387::ChipRev::Bm1387
                )))?
            }
            self.chip_count += 1;
        }

        if self.chip_count >= MAX_CHIPS_ON_CHAIN {
            Err(ErrorKind::Hashchip(format!(
                "detected {} chips, expected less than 256 chips on 1 chain. Possibly a hardware issue?",
                self.chip_count
            )))?
        }
        if self.chip_count == 0 {
            Err(ErrorKind::Hashchip(
                "no chips detected on the current chain".to_string(),
            ))?
        }
        // Set all chips to be offline before address assignment. This is important so that each
        // chip after initially accepting the address will pass on further addresses down the chain
        let inactivate_from_chain_cmd = bm1387::InactivateFromChainCmd::new().pack();
        // make sure all chips receive inactivation request
        for _ in 0..3 {
            self.send_ctl_cmd(&inactivate_from_chain_cmd, false);
            thread::sleep(Duration::from_millis(INACTIVATE_FROM_CHAIN_DELAY_MS));
        }

        // Assign address to each chip
        self.for_all_chips(|addr| {
            let cmd = bm1387::SetChipAddressCmd::new(addr);
            self.send_ctl_cmd(&cmd.pack(), false);
            Ok(())
        })?;

        Ok(())
    }

    /// Helper method that applies a function to all detected chips on the chain
    fn for_all_chips<F, R>(&self, f: F) -> error::Result<R>
    where
        F: Fn(u8) -> error::Result<R>,
    {
        let mut result = Err(ErrorKind::Hashchip("no chips to iterate".to_string()).into());
        for addr in (0..self.chip_count * 4).step_by(4) {
            // the enumeration takes care that address always fits into 8 bits.
            // Therefore, we can truncate the bits here.
            result = Ok(f(addr as u8)?);
        }
        // Result of last iteration
        result
    }

    /// Loads PLL register with a starting value
    fn set_pll(&self) -> error::Result<()> {
        self.for_all_chips(|addr| {
            // TODO: fix endianity of this register so it matches datasheet
            let cmd =
                bm1387::SetConfigCmd::new(addr, false, bm1387::PLL_PARAM_REG, DEFAULT_PLL_CONFIG);
            self.send_ctl_cmd(&cmd.pack(), false);
            Ok(())
        })
    }

    /// Configure all chips in the hash chain
    ///
    /// This method programs the MiscCtrl register of each chip in the hash chain.
    ///
    /// * `baud_rate` - desired communication speed
    /// * `not_set_baud` - the baud clock divisor is calculated, however, each chip will ignore
    /// its value. This is used typically when gate_block is enabled.
    /// * `gate_block` - allows gradual startup of the chips in the chain as they keep receiving
    /// special 'null' job. See bm1387::MiscCtrlReg::gate_block for details
    ///
    /// Returns actual baud rate that has been set on the chips or an error
    /// @todo Research the exact use case of 'not_set_baud' in conjunction with gate_block
    fn configure_hash_chain(
        &self,
        baud_rate: usize,
        not_set_baud: bool,
        gate_block: bool,
    ) -> error::Result<usize> {
        let (baud_clock_div, actual_baud_rate) = calc_baud_clock_div(
            baud_rate,
            CHIP_OSC_CLK_HZ,
            bm1387::CHIP_OSC_CLK_BASE_BAUD_DIV,
        )?;
        info!(
            "Setting Hash chain baud rate @ requested: {}, actual: {}, divisor {:#04x}",
            baud_rate, actual_baud_rate, baud_clock_div
        );
        // Each chip is always configured with inverted clock
        let ctl_reg =
            bm1387::MiscCtrlReg::new(not_set_baud, true, baud_clock_div, gate_block, true)?;
        // TODO: rework the setconfig::new interface to accept the register directly and
        // eliminate the register address in this place
        let cmd = bm1387::SetConfigCmd::new(0, true, bm1387::MISC_CONTROL_REG, ctl_reg.into());
        // wait until all commands have been sent
        self.send_ctl_cmd(&cmd.pack(), true);
        Ok(actual_baud_rate)
    }

    /// This method only changes the communication speed of the FPGA IP core with the chips.
    ///
    /// Note: change baud rate of the FPGA is only desirable as a step after all chips in the
    /// chain have been reconfigured for a different speed, too.
    fn set_ip_core_baud_rate(&self, baud: usize) -> error::Result<()> {
        let (baud_clock_div, actual_baud_rate) = calc_baud_clock_div(
            baud,
            FPGA_IPCORE_F_CLK_SPEED_HZ,
            FPGA_IPCORE_F_CLK_BASE_BAUD_DIV,
        )?;
        info!(
            "Setting IP core baud rate @ requested: {}, actual: {}, divisor {:#04x}",
            baud, actual_baud_rate, baud_clock_div
        );

        self.cmd_fifo.set_baud_clock_div(baud_clock_div as u32);
        Ok(())
    }

    /// Serializes command into 32-bit words and submits it to the command TX FIFO
    ///
    /// * `wait` - when true, wait until all commands are sent
    fn send_ctl_cmd(&self, cmd: &[u8], wait: bool) {
        // invariant required by the IP core
        assert_eq!(
            cmd.len() & 0x3,
            0,
            "Control command length not aligned to 4 byte boundary!"
        );
        trace!("Sending Control Command {:x?}", cmd);
        for chunk in cmd.chunks(4) {
            self.cmd_fifo
                .write_to_cmd_tx_fifo(LittleEndian::read_u32(chunk));
        }
        // TODO busy waiting has to be replaced once asynchronous processing is in place
        if wait {
            self.cmd_fifo.wait_cmd_tx_fifo_empty();
        }
    }

    /// Command responses are always 7 bytes long including checksum. Therefore, the reception
    /// has to be done in 2 steps with the following error handling:
    ///
    /// - A timeout when reading the first word is converted into an empty response.
    ///   The method propagates any error other than timeout
    /// - An error that occurs during reading the second word from the FIFO is propagated.
    fn recv_ctl_cmd_resp<T: PackedStructSlice>(&mut self) -> error::Result<Option<T>> {
        let mut cmd_resp = [0u8; 8];

        // TODO: to be refactored once we have asynchronous handling in place
        // fetch command response from IP core's fifo
        match self.cmd_fifo.read_from_cmd_rx_fifo()? {
            None => return Ok(None),
            Some(resp_word) => LittleEndian::write_u32(&mut cmd_resp[..4], resp_word),
        }
        // All errors from reading the second word are propagated
        match self.cmd_fifo.read_from_cmd_rx_fifo()? {
            None => {
                return Err(ErrorKind::Fifo(
                    error::Fifo::TimedOut,
                    "work RX fifo empty".to_string(),
                ))?;
            }
            Some(resp_word) => LittleEndian::write_u32(&mut cmd_resp[4..], resp_word),
        }

        // build the response instance - drop the extra byte due to FIFO being 32-bit word based
        // and drop the checksum
        // TODO: optionally verify the checksum (use debug_assert?)
        let resp = T::unpack_from_slice(&cmd_resp[..6]).context(format!(
            "control command unpacking error! {:#04x?}",
            cmd_resp
        ))?;
        Ok(Some(resp))
    }

    pub fn get_chip_count(&self) -> usize {
        self.chip_count
    }

    /// Initialize cores by sending open-core work with correct nbits to each core
    async fn send_init_work(h_chain_ctl: Arc<Mutex<Self>>, tx_fifo: &mut fifo::HChainFifo) {
        // Each core gets one work
        const NUM_WORK: usize = bm1387::NUM_CORES_ON_CHIP;
        trace!(
            "Sending out {} pieces of dummy work to initialize chips",
            NUM_WORK
        );
        for _ in 0..NUM_WORK {
            let work = &null_work::prepare_opencore(true, tx_fifo.midstate_count.to_count());
            let work_id = await!(h_chain_ctl.lock())
                .expect("h_chain lock")
                .work_id_gen
                .next();
            await!(tx_fifo.async_wait_for_work_tx_room()).expect("wait for tx room");
            // TODO: remember work_id assignment in registry
            tx_fifo.send_work(&work, work_id as u32).expect("send work");
        }
    }

    /// Generates enough testing work until the work FIFO becomes full
    /// The work is made unique by specifying a unique midstate.
    ///
    /// As the next step the method starts collecting solutions, eliminating duplicates and extracting
    /// valid solutions for further processing
    ///
    /// Returns the amount of work generated during this run
    async fn async_send_work(
        h_chain_ctl: Arc<Mutex<Self>>,
        work_registry: Arc<Mutex<registry::MiningWorkRegistry>>,
        mining_stats: Arc<Mutex<hal::MiningStats>>,
        mut tx_fifo: fifo::HChainFifo,
        mut work_generator: work::Generator,
    ) {
        loop {
            await!(tx_fifo.async_wait_for_work_tx_room()).expect("wait for tx room");
            let work = await!(work_generator.generate());
            match work {
                None => return,
                Some(work) => {
                    let work_id = await!(h_chain_ctl.lock())
                        .expect("h_chain lock")
                        .work_id_gen
                        .next();
                    // send work is synchronous
                    tx_fifo.send_work(&work, work_id as u32).expect("send work");
                    await!(work_registry.lock())
                        .expect("locking ok")
                        .store_work(work_id as usize, work);
                    let mut stats = await!(mining_stats.lock()).expect("minig stats lock");
                    stats.work_generated += tx_fifo.midstate_count.to_count();
                    drop(stats);
                }
            }
        }
    }

    async fn async_recv_solutions(
        _h_chain_ctl: Arc<Mutex<Self>>,
        work_registry: Arc<Mutex<registry::MiningWorkRegistry>>,
        mining_stats: Arc<Mutex<hal::MiningStats>>,
        mut rx_fifo: fifo::HChainFifo,
        solution_sender: work::SolutionSender,
    ) {
        // solution receiving/filtering part
        loop {
            let (rx_fifo_out, solution) =
                await!(rx_fifo.recv_solution()).expect("recv solution failed");
            rx_fifo = rx_fifo_out;
            let solution_id = SolutionId::from_reg(solution.solution_id, rx_fifo.midstate_count);
            let work_id = solution_id.work_id;
            let mut stats = await!(mining_stats.lock()).expect("lock mining stats");
            let mut work_registry =
                await!(work_registry.lock()).expect("work registry lock failed");

            let work = work_registry.find_work(work_id);
            match work {
                Some(work_item) => {
                    let status = work_item.insert_solution(solution);

                    // work item detected a new unique solution, we will push it for further processing
                    if let Some(unique_solution) = status.unique_solution {
                        if !status.duplicate {
                            if !unique_solution.is_valid(&ASIC_TARGET) {
                                warn!("Solution from hashchain not hitting ASIC target");
                                stats.error_stats.hardware_errors += 1;
                            }
                            solution_sender.send(unique_solution);
                        }
                    }
                    if status.duplicate {
                        stats.error_stats.duplicate_solutions += 1;
                    } else {
                        stats.unique_solutions += 1;
                        stats.unique_solutions_shares += config::ASIC_DIFFICULTY as u64;
                    }
                    if status.mismatched_nonce {
                        stats.error_stats.mismatched_solution_nonces += 1;
                    }
                }
                None => {
                    info!(
                        "No work present for solution, ID:{:#x} {:#010x?}",
                        work_id, solution
                    );
                    stats.error_stats.stale_solutions += 1;
                }
            }
        }
    }

    fn spawn_tx_task(
        h_chain_ctl: Arc<Mutex<Self>>,
        work_registry: Arc<Mutex<registry::MiningWorkRegistry>>,
        mining_stats: Arc<Mutex<hal::MiningStats>>,
        work_generator: work::Generator,
        shutdown: hal::ShutdownSender,
    ) {
        ii_async_compat::spawn(async move {
            let mut tx_fifo = await!(h_chain_ctl.lock())
                .expect("locking failed")
                .work_tx_fifo
                .take()
                .expect("work-tx fifo missing");

            await!(Self::send_init_work(h_chain_ctl.clone(), &mut tx_fifo));
            await!(Self::async_send_work(
                h_chain_ctl,
                work_registry,
                mining_stats,
                tx_fifo,
                work_generator,
            ));
            shutdown.send("no more work from workhub");
        });
    }

    fn spawn_rx_task(
        h_chain_ctl: Arc<Mutex<Self>>,
        work_registry: Arc<Mutex<registry::MiningWorkRegistry>>,
        mining_stats: Arc<Mutex<hal::MiningStats>>,
        solution_sender: work::SolutionSender,
    ) {
        ii_async_compat::spawn(async move {
            let rx_fifo = await!(h_chain_ctl.lock())
                .expect("locking failed")
                .work_rx_fifo
                .take()
                .expect("work-rx fifo missing");
            await!(Self::async_recv_solutions(
                h_chain_ctl,
                work_registry,
                mining_stats,
                rx_fifo,
                solution_sender,
            ));
        });
    }
}

pub struct HChain {}

impl HChain {
    pub fn new() -> Self {
        Self {}
    }

    pub fn start_h_chain(
        &self,
        work_solver: work::Solver,
        mining_stats: Arc<Mutex<hal::MiningStats>>,
        shutdown: hal::ShutdownSender,
        midstate_count: usize,
    ) -> Arc<
        Mutex<
            s9::HChainCtl<
                power::VoltageCtrlI2cSharedBlockingBackend<power::VoltageCtrlI2cBlockingBackend>,
            >,
        >,
    > {
        use s9::power::VoltageCtrlBackend;

        let gpio_mgr = gpio::ControlPinManager::new();
        let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::new(0);
        let voltage_ctrl_backend =
            power::VoltageCtrlI2cSharedBlockingBackend::new(voltage_ctrl_backend);
        let mut h_chain_ctl = s9::HChainCtl::new(
            &gpio_mgr,
            voltage_ctrl_backend.clone(),
            config::S9_HASHBOARD_INDEX,
            MidstateCount::new(midstate_count),
            config::ASIC_DIFFICULTY,
        )
        .unwrap();

        info!(
            "Initializing hash chain controller for (midstate count {})",
            midstate_count,
        );
        h_chain_ctl.init().unwrap();
        info!("Hash chain controller initialized");

        let work_registry = Arc::new(Mutex::new(registry::MiningWorkRegistry::new(
            midstate_count,
        )));
        let h_chain_ctl = Arc::new(Mutex::new(h_chain_ctl));
        let (work_generator, work_solution) = work_solver.split();

        HChainCtl::spawn_tx_task(
            h_chain_ctl.clone(),
            work_registry.clone(),
            mining_stats.clone(),
            work_generator,
            shutdown.clone(),
        );
        HChainCtl::spawn_rx_task(
            h_chain_ctl.clone(),
            work_registry.clone(),
            mining_stats.clone(),
            work_solution,
        );

        h_chain_ctl
    }
}

/// Entry point for running the hardware backend
pub fn run(
    work_solver: work::Solver,
    mining_stats: Arc<Mutex<hal::MiningStats>>,
    shutdown: hal::ShutdownSender,
) {
    // Create one chain
    let chain = HChain::new();
    chain.start_h_chain(
        work_solver,
        mining_stats,
        shutdown,
        runtime_config::get_midstate_count(),
    );
}

/// Helper method that calculates baud rate clock divisor value for the specified baud rate.
///
/// The calculation follows the same scheme for the hashing chips as well as for the FPGA IP core
///
/// * `baud_rate` - requested baud rate
/// * `base_clock_hz` - base clock for the UART peripheral
/// * `base_clock_div` - divisor for the base clock
/// Return a baudrate divisor and actual baud rate or an error
fn calc_baud_clock_div(
    baud_rate: usize,
    base_clock_hz: usize,
    base_clock_div: usize,
) -> error::Result<(usize, usize)> {
    const MAX_BAUD_RATE_ERR_PERC: usize = 5;
    // The actual calculation is:
    // base_clock_hz / (base_clock_div * baud_rate) - 1
    // We have to mathematically round the calculated divisor in fixed point arithmethic
    let baud_div = (10 * base_clock_hz / (base_clock_div * baud_rate) + 5) / 10 - 1;
    let actual_baud_rate = base_clock_hz / (base_clock_div * (baud_div + 1));

    //
    let baud_rate_diff = if actual_baud_rate > baud_rate {
        actual_baud_rate - baud_rate
    } else {
        baud_rate - actual_baud_rate
    };
    // the baud rate has to be within a few percents
    if baud_rate_diff > (MAX_BAUD_RATE_ERR_PERC * baud_rate / 100) {
        Err(ErrorKind::BaudRate(format!(
            "requested {} baud, resulting {} baud",
            baud_rate, actual_baud_rate
        )))?
    }
    Ok((baud_div, actual_baud_rate))
}

/// Helper method to calculate time to finish one piece of work
///
/// * `n_midstates` - number of midstates
/// * `pll_frequency` - frequency of chip in Hz
/// Return a number of seconds.
///
/// The formula for work_delay is:
///
///   work_delay = space_size_of_one_work / computation_speed; [sec, hashes, hashes_per_sec]
///
/// In our case it would be
///
///   work_delay = n_midstates * 2^32 / (freq * num_chips * cores_per_chip)
///
/// Unfortunately the space is not divided evenly, some nonces get never computed.
/// The current conjecture is that nonce space is divided by chip/core address,
/// ie. chip number 0x1a iterates all nonces 0x1axxxxxx. That's 6 bits of chip_address
/// and 7 bits of core_address. Putting it all together:
///
///   work_delay = n_midstates * num_chips * cores_per_chip * 2^(32 - 7 - 6) / (freq * num_chips * cores_per_chip)
///
/// Simplify:
///
///   work_delay = n_midstates * 2^19 / freq
///
/// Last but not least, we apply fudge factor of 0.9 and send work 11% faster to offset
/// delays when sending out/generating work/chips not getting proper work...:
///
///   work_delay = 0.9 * n_midstates * 2^19 / freq
fn calculate_work_delay_for_pll(n_midstates: u64, pll_frequency: u64) -> f64 {
    let space_size_per_core: u64 = 1 << 19;
    0.9 * (n_midstates * space_size_per_core) as f64 / pll_frequency as f64
}

/// Helper method to convert seconds to FPGA ticks suitable to be written
/// to `WORK_TIME` FPGA register.
///
/// Returns number of ticks.
fn secs_to_fpga_ticks(secs: f64) -> u32 {
    (secs * FPGA_IPCORE_F_CLK_SPEED_HZ as f64) as u32
}
