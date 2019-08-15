mod bm1387;
pub mod error;
pub mod fifo;
pub mod gpio;
pub mod null_work;
pub mod power;
pub mod registry;

// TODO: remove thread specific components
use std::default::Default;
use std::sync::Arc;
use std::thread;

use std::time::{Duration, SystemTime};

use tokio::await;
use wire::utils::CompatFix;

use slog::{info, trace};

use error::ErrorKind;
use failure::ResultExt;

use crate::misc::LOGGER;
use crate::work;

use futures_locks::Mutex;

use byteorder::{ByteOrder, LittleEndian};
use packed_struct::{PackedStruct, PackedStructSlice};

use embedded_hal::digital::v2::InputPin;
use embedded_hal::digital::v2::OutputPin;

/// How many work use to initialize the chain, use at least MAX_CHIPS_ON_CHAIN work
/// TODO: compare this to bitmain's open_core
const NUM_WORK_TO_OPEN_CORE: usize = 80;

/// Timing constants
const INACTIVATE_FROM_CHAIN_DELAY_MS: u64 = 100;
/// Base delay quantum during hashboard initialization
const INIT_DELAY_MS: u64 = 1000;

/// Maximum number of chips is limitted by the fact that there is only 8-bit address field and
/// addresses to the chips need to be assigned with step of 4 (e.g. 0, 4, 8, etc.)
const MAX_CHIPS_ON_CHAIN: usize = 64;

/// Bit position where work ID starts in the second word provided by the IP core with mining work
/// solution
const WORK_ID_OFFSET: usize = 8;

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

/// Hash Chain Options is to pass additional information during hash chain
/// controller initialization.
#[derive(Clone)]
pub struct HChainOptions {
    // This affects if initialization sends opencore-like jobs to init the chips.
    // Obviously unless you test opencore, this has to be enabled.
    pub send_init_work: bool,
    pub filter_duplicate_solutions: bool,
}

impl Default for HChainOptions {
    fn default() -> Self {
        Self {
            send_init_work: true,
            filter_duplicate_solutions: true,
        }
    }
}

/// Default ASIC difficulty
const ASIC_DIFFICULTY: u32 = 64;

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
    work_id: u16,
    /// Number of chips that have been detected
    chip_count: usize,
    /// Eliminates the need to query the IP core about the current number of configured midstates
    midstate_count_bits: u8,
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
    pub fn new(
        gpio_mgr: &gpio::ControlPinManager,
        voltage_ctrl_backend: VBackend,
        hashboard_idx: usize,
        midstate_count: &s9_io::hchainio0::ctrl_reg::MIDSTATE_CNTW,
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

        let midstate_count_bits = midstate_count._bits();
        Ok(Self {
            work_id: 0,
            chip_count: 0,
            midstate_count_bits,
            voltage_ctrl: power::VoltageCtrl::new(voltage_ctrl_backend, hashboard_idx),
            plug_pin,
            rst_pin,
            hashboard_idx,
            last_heartbeat_sent: None,
            // TODO: implement setting me
            pll_frequency: DEFAULT_S9_PLL_FREQUENCY,
            cmd_fifo: fifo::HChainFifo::new(hashboard_idx)?,
            work_rx_fifo: Some(fifo::HChainFifo::new(hashboard_idx)?),
            work_tx_fifo: Some(fifo::HChainFifo::new(hashboard_idx)?),
        })
    }

    /// Return number of midstates
    #[inline]
    fn midstate_count(&self) -> u16 {
        1 << self.midstate_count_bits
    }

    /// Calculate work_time for this instance of HChain
    ///
    /// Returns number of ticks (suitable to be written to `WORK_TIME` register)
    #[inline]
    fn calculate_work_time(&self) -> u32 {
        secs_to_fpga_ticks(calculate_work_delay_for_pll(
            self.midstate_count() as u64,
            self.pll_frequency,
        ))
    }

    /// Helper method that initializes the FPGA IP core
    fn ip_core_init(&self) -> error::Result<()> {
        // Disable ip core
        self.cmd_fifo.disable_ip_core();
        self.cmd_fifo.enable_ip_core();

        self.set_ip_core_baud_rate(INIT_CHIP_BAUD_RATE)?;
        let work_time = self.calculate_work_time();
        trace!(LOGGER, "Using work time: {}", work_time);
        self.cmd_fifo.set_ip_core_work_time(work_time);
        self.cmd_fifo
            .set_ip_core_midstate_count(self.midstate_count_bits);

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
    fn set_asic_diff(&self) -> error::Result<()> {
        let tm_reg = bm1387::TicketMaskReg::new(ASIC_DIFFICULTY)?;
        trace!(
            LOGGER,
            "Setting ticket mask register for difficulty {}, value {:#010x?}",
            ASIC_DIFFICULTY,
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
        info!(LOGGER, "Hashboard IP core initialized");
        self.voltage_ctrl.reset()?;
        info!(LOGGER, "Voltage controller reset");
        self.voltage_ctrl.jump_from_loader_to_app()?;
        info!(LOGGER, "Voltage controller application started");
        let version = self.voltage_ctrl.get_version()?;
        info!(
            LOGGER,
            "Voltage controller firmware version {:#04x}", version
        );
        // TODO accept multiple
        if version != power::EXPECTED_VOLTAGE_CTRL_VERSION {
            // TODO: error!(LOGGER, "{}", err_msg);
            Err(ErrorKind::UnexpectedVersion(
                "voltage controller firmware".to_string(),
                version.to_string(),
                power::EXPECTED_VOLTAGE_CTRL_VERSION.to_string(),
            ))?
        }
        // Voltage controller successfully initialized at this point, we should start sending
        // heart beats to it. Otherwise, it would shut down in about 10 seconds.
        info!(LOGGER, "Starting voltage controller heart beat task");
        let _ = self.voltage_ctrl.start_heart_beat_task();

        self.voltage_ctrl.set_voltage(6)?;
        self.voltage_ctrl.enable_voltage()?;
        info!(LOGGER, "Resetting hash board");
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
        info!(LOGGER, "Starting chip enumeration");
        self.enumerate_chips()?;
        info!(LOGGER, "Discovered {} chips", self.chip_count);

        // set PLL
        self.set_pll()?;

        // configure the hashing chain to operate at desired baud rate. Note that gate block is
        // enabled to allow continuous start of chips in the chain
        self.configure_hash_chain(TARGET_CHIP_BAUD_RATE, false, true)?;
        self.set_ip_core_baud_rate(TARGET_CHIP_BAUD_RATE)?;

        self.set_asic_diff()?;
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
            // see {"650",0x068040, 0x0220, 0x680221} in driver-btm-c5
            // TODO: fix endianity of this register so it matches datasheet
            let cmd = bm1387::SetConfigCmd::new(addr, false, bm1387::PLL_PARAM_REG, 0x21026800);
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
            LOGGER,
            "Setting Hash chain baud rate @ requested: {}, actual: {}, divisor {:#04x}",
            baud_rate,
            actual_baud_rate,
            baud_clock_div
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
            LOGGER,
            "Setting IP core baud rate @ requested: {}, actual: {}, divisor {:#04x}",
            baud,
            actual_baud_rate,
            baud_clock_div
        );

        self.cmd_fifo.set_baud_clock_div(baud_clock_div as u32);
        Ok(())
    }

    /// Work ID's are generated with a step that corresponds to the number of configured midstates
    #[inline]
    pub fn next_work_id(&mut self) -> u32 {
        let retval = self.work_id as u32;
        // compiler has to know that work ID rolls over regularly
        self.work_id = self.work_id.wrapping_add(self.midstate_count());
        retval
    }

    /// Helper function that extracts work ID from the solution ID
    #[inline]
    pub fn get_work_id_from_solution_id(&self, solution_id: u32) -> u32 {
        ((solution_id >> WORK_ID_OFFSET) >> self.midstate_count_bits)
    }

    /// Extracts midstate index from the solution ID
    #[inline]
    fn get_midstate_idx_from_solution_id(&self, solution_id: u32) -> usize {
        ((solution_id >> WORK_ID_OFFSET) & ((1u32 << self.midstate_count_bits) - 1)) as usize
    }

    /// Extracts solution index from the solution ID
    #[inline]
    pub fn get_solution_idx_from_solution_id(&self, solution_id: u32) -> usize {
        (solution_id & ((1u32 << WORK_ID_OFFSET) - 1)) as usize
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
        trace!(LOGGER, "Sending Control Command {:x?}", cmd);
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

    fn get_work_id_from_solution(&self, solution: &super::MiningWorkSolution) -> u32 {
        self.get_work_id_from_solution_id(solution.solution_id)
    }

    pub fn get_chip_count(&self) -> usize {
        self.chip_count
    }

    async fn recv_solution(
        h_chain_ctl: Arc<Mutex<Self>>,
        mut rx_fifo: fifo::HChainFifo,
    ) -> Result<(fifo::HChainFifo, Option<crate::hal::MiningWorkSolution>), failure::Error> {
        let nonce = await!(rx_fifo.async_read_from_work_rx_fifo())?;
        let word2 = await!(rx_fifo.async_read_from_work_rx_fifo())?;

        let h_chain_ctl = await!(h_chain_ctl.lock()).expect("h_chain lock failed");
        let solution = crate::hal::MiningWorkSolution {
            nonce,
            // this hardware doesn't do any nTime rolling, keep it @ None
            ntime: None,
            midstate_idx: h_chain_ctl.get_midstate_idx_from_solution_id(word2),
            // leave the result ID as-is so that we can extract solution index etc later.
            solution_id: word2 & 0xffffffu32,
        };

        Ok((rx_fifo, Some(solution)))
    }

    fn send_work(
        tx_fifo: &mut fifo::HChainFifo,
        work: &crate::hal::MiningWork,
        work_id: u32,
    ) -> Result<u32, failure::Error> {
        tx_fifo.write_to_work_tx_fifo(work_id.to_le())?;
        tx_fifo.write_to_work_tx_fifo(work.bits().to_le())?;
        tx_fifo.write_to_work_tx_fifo(work.ntime.to_le())?;
        tx_fifo.write_to_work_tx_fifo(work.merkle_root_tail().to_le())?;

        for mid in work.midstates.iter() {
            for midstate_word in mid.state.words::<u32>().rev() {
                tx_fifo.write_to_work_tx_fifo(midstate_word.to_be())?;
            }
        }
        Ok(work_id)
    }
}

async fn send_init_work<T>(tx_fifo: &mut fifo::HChainFifo)
where
    T: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    // initialize chip by sending test work with correct difficulty (0xffffffff works)
    // TODO: use fixed job copied from bitmain's cgmminer, prepare_test_work may change
    trace!(
        LOGGER,
        "Sending out {} pieces of dummy work to initialize chips",
        NUM_WORK_TO_OPEN_CORE
    );
    for i in 0..NUM_WORK_TO_OPEN_CORE {
        let work = &null_work::prepare(0);
        await!(tx_fifo.async_wait_for_work_tx_room()).expect("wait for tx room");
        super::s9::HChainCtl::<T>::send_work(tx_fifo, &work, i as u32).expect("send work");
    }
}

/// Generates enough testing work until the work FIFO becomes full
/// The work is made unique by specifying a unique midstate.
///
/// As the next step the method starts collecting solutions, eliminating duplicates and extracting
/// valid solutions for further processing
///
/// Returns the amount of work generated during this run

async fn async_send_work<T>(
    work_registry: Arc<Mutex<registry::MiningWorkRegistry>>,
    mining_stats: Arc<Mutex<super::MiningStats>>,
    h_chain_ctl: Arc<Mutex<super::s9::HChainCtl<T>>>,
    mut tx_fifo: fifo::HChainFifo,
    mut work_generator: work::Generator,
    opts: HChainOptions,
) where
    T: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    if opts.send_init_work {
        await!(send_init_work::<T>(&mut tx_fifo));
    }
    loop {
        await!(tx_fifo.async_wait_for_work_tx_room()).expect("wait for tx room");
        let work = await!(work_generator.generate());
        match work {
            None => return,
            Some(work) => {
                let work_id = await!(h_chain_ctl.lock())
                    .expect("h_chain lock")
                    .next_work_id();
                // send work is synchronous
                super::s9::HChainCtl::<T>::send_work(&mut tx_fifo, &work, work_id)
                    .expect("send work");
                await!(work_registry.lock())
                    .expect("locking ok")
                    .store_work(work_id as usize, work);
                let mut stats = await!(mining_stats.lock()).expect("minig stats lock");
                stats.work_generated += 1;
                drop(stats);
            }
        }
    }
}

async fn async_recv_solutions<T>(
    work_registry: Arc<Mutex<registry::MiningWorkRegistry>>,
    mining_stats: Arc<Mutex<super::MiningStats>>,
    h_chain_ctl: Arc<Mutex<super::s9::HChainCtl<T>>>,
    mut rx_fifo: fifo::HChainFifo,
    work_solution: work::SolutionSender,
    opts: HChainOptions,
) where
    T: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    // solution receiving/filtering part
    loop {
        let (rx_fifo_out, solution) = await!(super::s9::HChainCtl::recv_solution(
            h_chain_ctl.clone(),
            rx_fifo
        ))
        .expect("recv solution");
        rx_fifo = rx_fifo_out;
        let solution = solution.expect("solution is ok");
        let work_id = await!(h_chain_ctl.lock())
            .expect("h_chain lock")
            .get_work_id_from_solution(&solution) as usize;
        let mut stats = await!(mining_stats.lock()).expect("lock mining stats");
        let mut work_registry = await!(work_registry.lock()).expect("work registry lock failed");

        let work = work_registry.find_work(work_id);
        match work {
            Some(work_item) => {
                let solution_idx = await!(h_chain_ctl.lock())
                    .expect("h_chain lock")
                    .get_solution_idx_from_solution_id(solution.solution_id);
                let status = work_item.insert_solution(solution, solution_idx);

                // work item detected a new unique solution, we will push it for further processing
                if let Some(unique_solution) = status.unique_solution {
                    if !status.duplicate || !opts.filter_duplicate_solutions {
                        work_solution.send(unique_solution);
                    }
                }
                if status.duplicate {
                    stats.duplicate_solutions += 1;
                } else {
                    stats.unique_solutions += 1;
                }
                stats.mismatched_solution_nonces += status.mismatched_nonce as u64;
            }
            None => {
                trace!(
                    LOGGER,
                    "No work present for solution, ID:{:#x} {:#010x?}",
                    work_id,
                    solution
                );
                stats.stale_solutions += 1;
            }
        }
    }
}

fn spawn_tx_task<T>(
    work_registry: Arc<Mutex<registry::MiningWorkRegistry>>,
    mining_stats: Arc<Mutex<super::MiningStats>>,
    h_chain_ctl: Arc<Mutex<super::s9::HChainCtl<T>>>,
    work_generator: work::Generator,
    shutdown: crate::hal::ShutdownSender,
    opts: HChainOptions,
) where
    T: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    tokio::spawn(
        async move {
            let mut tx_fifo = await!(h_chain_ctl.lock())
                .expect("locking failed")
                .work_tx_fifo
                .take()
                .expect("work-tx fifo missing");
            await!(async_send_work(
                work_registry,
                mining_stats,
                h_chain_ctl,
                tx_fifo,
                work_generator,
                opts,
            ));
            shutdown.send("no more work from workhub");
        }
            .compat_fix(),
    );
}

fn spawn_rx_task<T>(
    work_registry: Arc<Mutex<registry::MiningWorkRegistry>>,
    mining_stats: Arc<Mutex<super::MiningStats>>,
    h_chain_ctl: Arc<Mutex<super::s9::HChainCtl<T>>>,
    work_solution: work::SolutionSender,
    opts: HChainOptions,
) where
    T: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    tokio::spawn(
        async move {
            let mut rx_fifo = await!(h_chain_ctl.lock())
                .expect("locking failed")
                .work_rx_fifo
                .take()
                .expect("work-rx fifo missing");
            await!(async_recv_solutions(
                work_registry,
                mining_stats,
                h_chain_ctl,
                rx_fifo,
                work_solution,
                opts,
            ));
        }
            .compat_fix(),
    );
}

pub struct HChain {
    opts: HChainOptions,
}

impl HChain {
    pub fn new(opts: HChainOptions) -> Self {
        Self { opts }
    }

    pub fn start_h_chain(
        &self,
        work_solver: work::Solver,
        mining_stats: Arc<Mutex<super::MiningStats>>,
        shutdown: crate::hal::ShutdownSender,
    ) -> Arc<
        Mutex<
            super::s9::HChainCtl<
                power::VoltageCtrlI2cSharedBlockingBackend<power::VoltageCtrlI2cBlockingBackend>,
            >,
        >,
    > {
        use super::s9::power::VoltageCtrlBackend;

        let gpio_mgr = gpio::ControlPinManager::new();
        let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::new(0);
        let voltage_ctrl_backend =
            power::VoltageCtrlI2cSharedBlockingBackend::new(voltage_ctrl_backend);
        let mut h_chain_ctl = super::s9::HChainCtl::new(
            &gpio_mgr,
            voltage_ctrl_backend.clone(),
            8,
            &s9_io::hchainio0::ctrl_reg::MIDSTATE_CNTW::ONE,
        )
        .unwrap();

        info!(LOGGER, "Initializing hash chain controller");
        h_chain_ctl.init().unwrap();
        info!(LOGGER, "Hash chain controller initialized");

        let work_registry = Arc::new(Mutex::new(registry::MiningWorkRegistry::new()));
        let h_chain_ctl = Arc::new(Mutex::new(h_chain_ctl));
        let (work_generator, work_solution) = work_solver.split();

        spawn_tx_task(
            work_registry.clone(),
            mining_stats.clone(),
            h_chain_ctl.clone(),
            work_generator,
            shutdown.clone(),
            self.opts.clone(),
        );
        spawn_rx_task(
            work_registry.clone(),
            mining_stats.clone(),
            h_chain_ctl.clone(),
            work_solution,
            self.opts.clone(),
        );

        h_chain_ctl
    }
}

/// Entry point for running the hardware backend
pub fn run(
    work_solver: work::Solver,
    mining_stats: Arc<Mutex<super::MiningStats>>,
    shutdown: crate::hal::ShutdownSender,
) {
    // Create one chain
    let chain = HChain::new(Default::default());
    chain.start_h_chain(work_solver, mining_stats, shutdown);
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

#[cfg(test)]
mod test {
    use super::*;
    use s9_io::hchainio0;
    //    use std::sync::{Once, ONCE_INIT};
    //
    //    static H_CHAIN_CTL_INIT: Once = ONCE_INIT;
    //    static mut H_CHAIN_CTL: HChainCtl = HChainCtl {
    //
    //    };
    //
    //    fn get_ctl() -> Result<HChainCtl, io::Error>  {
    //        H_CHAIN_CTL.call_once(|| {
    //            let h_chain_ctl = HChainCtl::new();
    //        });
    //        h_chain_ctl
    //    }

    #[test]
    fn test_hchain_ctl_instance() {
        let gpio_mgr = gpio::ControlPinManager::new();
        let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::new(0);
        let h_chain_ctl = HChainCtl::new(
            &gpio_mgr,
            voltage_ctrl_backend,
            8,
            &hchainio0::ctrl_reg::MIDSTATE_CNTW::ONE,
        );
        match h_chain_ctl {
            Ok(_) => assert!(true),
            Err(e) => assert!(false, "Failed to instantiate hash chain, error: {}", e),
        }
    }

    #[test]
    fn test_hchain_ctl_init() {
        let gpio_mgr = gpio::ControlPinManager::new();
        let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::new(0);
        let h_chain_ctl = HChainCtl::new(
            &gpio_mgr,
            voltage_ctrl_backend,
            8,
            &hchainio0::ctrl_reg::MIDSTATE_CNTW::ONE,
        )
        .expect("Failed to create hash board instance");

        assert!(
            h_chain_ctl.ip_core_init().is_ok(),
            "Failed to initialize IP core"
        );

        // verify sane register values
        assert_eq!(
            h_chain_ctl.cmd_fifo.hash_chain_io.work_time.read().bits(),
            36296,
            "Unexpected work time value"
        );
        assert_eq!(
            h_chain_ctl.cmd_fifo.hash_chain_io.baud_reg.read().bits(),
            0x1a,
            "Unexpected baud rate register value for {} baud",
            INIT_CHIP_BAUD_RATE
        );
        assert_eq!(
            h_chain_ctl.cmd_fifo.hash_chain_io.stat_reg.read().bits(),
            0x855,
            "Unexpected status register value"
        );
        assert_eq!(
            h_chain_ctl
                .cmd_fifo
                .hash_chain_io
                .ctrl_reg
                .read()
                .midstate_cnt(),
            hchainio0::ctrl_reg::MIDSTATE_CNTR::ONE,
            "Unexpected midstate count"
        );
    }

    /// This test verifies correct parsing of mining work solution for all multi-midstate
    /// configurations.
    /// The solution_word represents the second word of data provided that follows the nonce as
    /// provided by the FPGA IP core
    #[test]
    fn test_get_solution_word_attributes() {
        let solution_word = 0x00123502;
        struct ExpectedSolutionData {
            work_id: u32,
            midstate_idx: usize,
            solution_idx: usize,
            midstate_count: hchainio0::ctrl_reg::MIDSTATE_CNTW,
        };
        let expected_solution_data = [
            ExpectedSolutionData {
                work_id: 0x1235,
                midstate_idx: 0,
                solution_idx: 2,
                midstate_count: hchainio0::ctrl_reg::MIDSTATE_CNTW::ONE,
            },
            ExpectedSolutionData {
                work_id: 0x1235 >> 1,
                midstate_idx: 1,
                solution_idx: 2,
                midstate_count: hchainio0::ctrl_reg::MIDSTATE_CNTW::TWO,
            },
            ExpectedSolutionData {
                work_id: 0x1235 >> 2,
                midstate_idx: 1,
                solution_idx: 2,
                midstate_count: hchainio0::ctrl_reg::MIDSTATE_CNTW::FOUR,
            },
        ];
        for (i, expected_solution_data) in expected_solution_data.iter().enumerate() {
            // The midstate configuration (ctrl_reg::MIDSTATE_CNTW) doesn't implement a debug
            // trait. Therefore, we extract only those parts that can be easily displayed when a
            // test failed.
            let expected_data = (
                expected_solution_data.work_id,
                expected_solution_data.midstate_idx,
                expected_solution_data.solution_idx,
            );
            let gpio_mgr = gpio::ControlPinManager::new();
            let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::new(0);
            let h_chain_ctl = HChainCtl::new(
                &gpio_mgr,
                voltage_ctrl_backend,
                8,
                &expected_solution_data.midstate_count,
            )
            .unwrap();

            assert_eq!(
                h_chain_ctl.get_work_id_from_solution_id(solution_word),
                expected_solution_data.work_id,
                "Invalid work ID, iteration: {}, test data: {:#06x?}",
                i,
                expected_data
            );
            assert_eq!(
                h_chain_ctl.get_midstate_idx_from_solution_id(solution_word),
                expected_solution_data.midstate_idx,
                "Invalid midstate index, iteration: {}, test data: {:#06x?}",
                i,
                expected_data
            );
            assert_eq!(
                h_chain_ctl.get_solution_idx_from_solution_id(solution_word),
                expected_solution_data.solution_idx,
                "Invalid solution index, iteration: {}, test data: {:#06x?}",
                i,
                expected_data
            );
        }
    }
    #[test]
    fn test_calc_baud_div_correct_baud_rate_bm1387() {
        // these are sample baud rates for communicating with BM1387 chips
        let correct_bauds_and_divs = [
            (115_200usize, 26usize),
            (460_800, 6),
            (1_500_000, 1),
            (3_000_000, 0),
        ];
        for (baud_rate, baud_div) in correct_bauds_and_divs.iter() {
            let (baud_clock_div, actual_baud_rate) = calc_baud_clock_div(
                *baud_rate,
                CHIP_OSC_CLK_HZ,
                bm1387::CHIP_OSC_CLK_BASE_BAUD_DIV,
            )
            .unwrap();
            assert_eq!(
                baud_clock_div, *baud_div,
                "Calculated baud divisor doesn't match, requested: {} baud, actual: {} baud",
                baud_rate, actual_baud_rate
            )
        }
    }

    /// Test higher baud rate than supported
    #[test]
    fn test_calc_baud_div_over_baud_rate_bm1387() {
        let result = calc_baud_clock_div(
            3_500_000,
            CHIP_OSC_CLK_HZ,
            bm1387::CHIP_OSC_CLK_BASE_BAUD_DIV,
        );
        assert!(
            result.is_err(),
            "Baud clock divisor unexpectedly calculated!"
        );
    }

    /// Test work_time computation
    #[test]
    fn test_work_time_computation() {
        // you need to recalc this if you change asic diff or fpga freq
        assert_eq!(
            secs_to_fpga_ticks(calculate_work_delay_for_pll(1, 650_000_000)),
            36296
        );
    }
}
