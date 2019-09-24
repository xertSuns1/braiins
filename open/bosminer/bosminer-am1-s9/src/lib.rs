// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU Common Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Common Public License for more details.
//
// You should have received a copy of the GNU Common Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

#![feature(await_macro, async_await, duration_float)]

mod bm1387;
pub mod config;
pub mod error;
pub mod gpio;
pub mod io;
pub mod null_work;
pub mod power;
pub mod registry;
pub mod utils;

#[cfg(test)]
pub mod test;

use ii_logging::macros::*;

use bosminer::hal;
use bosminer::runtime_config;
use bosminer::shutdown;
use bosminer::stats;
use bosminer::work;

use std::sync::Arc;

use lazy_static::lazy_static;

use std::time::{Duration, SystemTime};

use error::ErrorKind;
use failure::ResultExt;

use futures::lock::Mutex;

use crate::utils::PackedRegister;
use packed_struct::{PackedStruct, PackedStructSlice};

use embedded_hal::digital::v2::InputPin;
use embedded_hal::digital::v2::OutputPin;

use ii_fpga_io_am1_s9::common::ctrl_reg::MIDSTATE_CNT_A;

use ii_async_compat::sleep;

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
const DEFAULT_S9_PLL_FREQUENCY: usize = 650_000_000;

/// Default initial voltage
const INITIAL_VOLTAGE: power::Voltage = power::Voltage::from_volts(9.4);

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

    /// Return log2 of midstate count
    #[inline]
    fn to_bits(&self) -> usize {
        self.log2
    }

    /// Return midstate count mask (to get midstate_idx bits from `work_id`)
    #[inline]
    fn to_mask(&self) -> usize {
        (1 << self.log2) - 1
    }
}

/// Enum representig chip address
#[derive(Debug, Clone, Copy)]
enum ChipAddress {
    All,
    One(u8),
}

impl ChipAddress {
    // is address broadcast?
    fn is_to_all(&self) -> bool {
        match self {
            ChipAddress::All => true,
            ChipAddress::One(_) => false,
        }
    }

    // get chip address or 0 if it's broadcast
    fn get_addr(&self) -> u8 {
        match self {
            ChipAddress::All => 0,
            ChipAddress::One(x) => *x,
        }
    }
}

/// Hash Chain Controller provides abstraction of the FPGA interface for operating hashing boards.
/// It is the user-space driver for the IP Core
///
/// Main responsibilities:
/// - memory mapping of the FPGA control interface
/// - mining work submission and solution processing
pub struct HashChain<VBackend> {
    /// Number of chips that have been detected
    chip_count: usize,
    /// Eliminates the need to query the IP core about the current number of configured midstates
    midstate_count: MidstateCount,
    /// ASIC difficulty
    asic_difficulty: usize,
    /// PLL frequency
    pll_frequency: usize,
    /// Voltage controller on this hashboard
    voltage_ctrl: power::Control<VBackend>,
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
    pub command_io: io::CommandRxTx,
    pub common_io: io::Common,
    pub work_rx_io: Option<io::WorkRx>,
    pub work_tx_io: Option<io::WorkTx>,
}

impl<VBackend> HashChain<VBackend>
where
    VBackend: 'static + Send + Sync + Clone + power::Backend,
{
    /// Creates a new hashboard controller with memory mapped FPGA IP core
    ///
    /// * `gpio_mgr` - gpio manager used for producing pins required for hashboard control
    /// * `voltage_ctrl_backend` - communication backend for the voltage controller
    /// * `hashboard_idx` - index of this hashboard determines which FPGA IP core is to be mapped
    /// * `midstate_count` - see Self
    /// * `asic_difficulty` - to what difficulty set the hardware target filter
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

        let core = io::Core::new(hashboard_idx, midstate_count)?;
        // Unfortunately, we have to do IP core re-init here (but it should be OK, it's synchronous)
        let (common_io, command_io, work_rx_io, work_tx_io) = core.init_and_split()?;

        Ok(Self {
            chip_count: 0,
            midstate_count,
            asic_difficulty,
            voltage_ctrl: power::Control::new(voltage_ctrl_backend, hashboard_idx),
            plug_pin,
            rst_pin,
            hashboard_idx,
            last_heartbeat_sent: None,
            pll_frequency: DEFAULT_S9_PLL_FREQUENCY,
            common_io,
            command_io,
            work_rx_io: Some(work_rx_io),
            work_tx_io: Some(work_tx_io),
        })
    }
    /// Calculate work_time for this instance of HChain
    ///
    /// Returns number of ticks (suitable to be written to `WORK_TIME` register)
    #[inline]
    fn calculate_work_time(&self) -> u32 {
        secs_to_fpga_ticks(calculate_work_delay_for_pll(
            self.midstate_count.to_count(),
            self.pll_frequency,
        ))
    }

    /// Helper method that initializes the FPGA IP core
    fn ip_core_init(&mut self) -> error::Result<()> {
        // Configure IP core
        self.set_ip_core_baud_rate(INIT_CHIP_BAUD_RATE)?;
        let work_time = self.calculate_work_time();
        trace!("Using work time: {}", work_time);
        self.common_io.set_ip_core_work_time(work_time);
        self.common_io.set_midstate_count();

        Ok(())
    }

    /// Puts the board into reset mode and disables the associated IP core
    fn enter_reset(&mut self) -> error::Result<()> {
        self.common_io.disable_ip_core();
        // perform reset of the hashboard
        self.rst_pin.set_low()?;
        Ok(())
    }

    /// Leaves reset mode
    fn exit_reset(&mut self) -> error::Result<()> {
        self.rst_pin.set_high()?;
        self.common_io.enable_ip_core();
        Ok(())
    }

    async fn read_register(&mut self, addr: ChipAddress, register: u8) -> error::Result<Vec<u32>> {
        let cmd = bm1387::GetStatusCmd::new(addr.get_addr(), addr.is_to_all(), register).pack();
        // send command, do not wait for it to be sent out
        await!(self.send_ctl_cmd(cmd.to_vec(), false));

        // wait for all responses and collect them
        let mut responses = Vec::new();
        loop {
            match await!(self.command_io.recv_response())? {
                Some(response) => {
                    let cmd_response = bm1387::CmdResponse::unpack_from_slice(&response)
                        .context(format!("response unpacking failed"))?;
                    responses.push(cmd_response.value);
                    // exit early if we expect just one reply
                    if !addr.is_to_all() {
                        break;
                    }
                }
                None => break,
            }
        }
        Ok(responses)
    }

    async fn write_register(
        &mut self,
        addr: ChipAddress,
        read_back: bool,
        register: u8,
        value: u32,
    ) -> error::Result<()> {
        let cmd = bm1387::SetConfigCmd::new(addr.get_addr(), addr.is_to_all(), register, value);
        // wait for command to be sent out
        await!(self.send_ctl_cmd(cmd.pack().to_vec(), true));

        // read the register back
        if read_back {
            let responses = await!(self.read_register(addr, register))?;
            // TODO: insert here some more checks - like does number of replies match number of chips etc.
            for (chip_address, read_back_value) in responses.iter().enumerate() {
                if *read_back_value != value {
                    Err(ErrorKind::Hashchip(format!(
                        "chip {} returned wrong value of register {:#x}: {:#x} instead of {:#x}",
                        chip_address, register, *read_back_value, value
                    )))?
                }
            }
        }

        Ok(())
    }

    /// Configures difficulty globally on all chips within the hashchain
    async fn set_asic_diff(&mut self, difficulty: usize) -> error::Result<()> {
        let tm_reg = bm1387::TicketMaskReg::new(difficulty as u32)?;
        trace!(
            "Setting ticket mask register for difficulty {}, value {:#010x?}",
            difficulty,
            tm_reg
        );
        await!(self.write_register(
            ChipAddress::All,
            true,
            bm1387::TICKET_MASK_REG,
            tm_reg.to_reg()
        ))?;
        Ok(())
    }

    /// Initializes the complete hashboard including enumerating all chips
    pub async fn init(&mut self) -> error::Result<()> {
        info!("Initializing hash chain {}", self.hashboard_idx);
        self.ip_core_init()?;
        info!("Hashboard IP core initialized");
        await!(self.voltage_ctrl.reset())?;
        info!("Voltage controller reset");
        await!(self.voltage_ctrl.jump_from_loader_to_app())?;
        info!("Voltage controller application started");
        let version = self.voltage_ctrl.get_version()?;
        info!("Voltage controller firmware version {:#04x}", version);
        // TODO accept multiple
        if version != power::EXPECTED_VOLTAGE_CTRL_VERSION {
            Err(ErrorKind::UnexpectedVersion(
                "voltage controller firmware".to_string(),
                version.to_string(),
                power::EXPECTED_VOLTAGE_CTRL_VERSION.to_string(),
            ))?
        }
        await!(self.voltage_ctrl.set_voltage(INITIAL_VOLTAGE))?;
        self.voltage_ctrl.enable_voltage()?;

        // Voltage controller successfully initialized at this point, we should start sending
        // heart beats to it. Otherwise, it would shut down in about 10 seconds.
        info!("Starting voltage controller heart beat task");
        let _ = self.voltage_ctrl.start_heart_beat_task();

        info!("Resetting hash board");
        self.enter_reset()?;
        // disable voltage
        self.voltage_ctrl.disable_voltage()?;
        await!(sleep(Duration::from_millis(INIT_DELAY_MS)));
        self.voltage_ctrl.enable_voltage()?;
        await!(sleep(Duration::from_millis(2 * INIT_DELAY_MS)));

        // TODO consider including a delay
        self.exit_reset()?;
        await!(sleep(Duration::from_millis(INIT_DELAY_MS)));
        //        let voltage = self.voltage_ctrl.get_voltage()?;
        //        if voltage != 0 {
        //            return Err(io::Error::new(
        //                io::ErrorKind::Other, format!("Detected voltage {}", voltage)));
        //        }
        info!("Starting chip enumeration");
        await!(self.enumerate_chips())?;
        info!("Discovered {} chips", self.chip_count);

        // calculate PLL for given frequency
        let pll = bm1387::Pll::try_pll_from_freq(CHIP_OSC_CLK_HZ, DEFAULT_S9_PLL_FREQUENCY)?;
        // set PLL
        await!(self.set_pll(pll))?;

        // configure the hashing chain to operate at desired baud rate. Note that gate block is
        // enabled to allow continuous start of chips in the chain
        await!(self.configure_hash_chain(TARGET_CHIP_BAUD_RATE, false, true))?;
        self.set_ip_core_baud_rate(TARGET_CHIP_BAUD_RATE)?;

        await!(self.set_asic_diff(self.asic_difficulty))?;
        Ok(())
    }

    /// Detects the number of chips on the hashing chain and assigns an address to each chip
    async fn enumerate_chips(&mut self) -> error::Result<()> {
        // Enumerate all chips (broadcast read address register request)
        let responses = await!(self.read_register(ChipAddress::All, bm1387::GET_ADDRESS_REG))?;

        // Check if are responses meaningful
        for (address, register) in responses.iter().enumerate() {
            let addr_reg = bm1387::GetAddressReg::from_reg(*register).expect("unpacking failed");
            if addr_reg.chip_rev != bm1387::ChipRev::Bm1387 {
                Err(ErrorKind::Hashchip(format!(
                    "unexpected revision of chip {} (expected: {:?} received: {:?})",
                    address,
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
            await!(self.send_ctl_cmd(inactivate_from_chain_cmd.to_vec(), false));
            await!(sleep(Duration::from_millis(INACTIVATE_FROM_CHAIN_DELAY_MS)));
        }

        // Assign address to each chip
        for addr in self.chip_iter() {
            let cmd = bm1387::SetChipAddressCmd::new(addr);
            await!(self.send_ctl_cmd(cmd.pack().to_vec(), false));
        }

        Ok(())
    }

    /// Returns iterator over all chips (yield their u8 addresses)
    fn chip_iter(&self) -> impl Iterator<Item = u8> {
        // make sure there is not too many chips
        assert!(self.chip_count * 4 < 256);
        (0..(self.chip_count as u8 * 4)).step_by(4)
    }

    /// Loads PLL register with a starting value
    async fn set_pll(&mut self, pll: bm1387::Pll) -> error::Result<()> {
        for addr in self.chip_iter() {
            // NOTE: when PLL register is read back, it is or-ed with 0x8000_0000, not sure why
            await!(self.write_register(
                ChipAddress::One(addr),
                false,
                bm1387::PLL_PARAM_REG,
                pll.to_reg()
            ))?;
        }
        Ok(())
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
    async fn configure_hash_chain(
        &mut self,
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
        await!(self.write_register(
            ChipAddress::All,
            true,
            bm1387::MISC_CONTROL_REG,
            ctl_reg.to_reg()
        ))?;
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

        self.common_io.set_baud_clock_div(baud_clock_div as u32);
        Ok(())
    }

    /// Receive command response and unpack it into struct T
    async fn recv_cmd_resp<T: PackedStructSlice>(&mut self) -> error::Result<Option<T>> {
        match await!(self.command_io.recv_response())? {
            Some(cmd_resp) => {
                let resp = T::unpack_from_slice(&cmd_resp).context(format!(
                    "control command unpacking error! {:#04x?}",
                    cmd_resp
                ))?;
                Ok(Some(resp))
            }
            None => Ok(None),
        }
    }

    async fn send_ctl_cmd(&self, cmd: Vec<u8>, wait: bool) {
        await!(self.command_io.send_command(cmd, wait));
    }

    pub fn get_chip_count(&self) -> usize {
        self.chip_count
    }

    /// Initialize cores by sending open-core work with correct nbits to each core
    async fn send_init_work(
        hash_chain: Arc<Mutex<Self>>,
        work_registry: Arc<Mutex<registry::WorkRegistry>>,
        tx_fifo: &mut io::WorkTx,
    ) {
        // Each core gets one work
        const NUM_WORK: usize = bm1387::NUM_CORES_ON_CHIP;
        trace!(
            "Sending out {} pieces of dummy work to initialize chips",
            NUM_WORK
        );
        let midstate_count = await!(hash_chain.lock()).midstate_count.to_count();
        for _ in 0..NUM_WORK {
            let work = &null_work::prepare_opencore(true, midstate_count);
            let work_id = await!(work_registry.lock()).store_work(work.clone());
            await!(tx_fifo.wait_for_room()).expect("wait for tx room");
            tx_fifo.send_work(&work, work_id).expect("send work");
        }
    }

    /// This task picks up work from frontend (via generator), saves it to
    /// registry (to pair with `Assignment` later) and sends it out to hw.
    /// It makes sure that TX fifo is empty before requesting work from
    /// generator.
    /// It exits when generator returns `None`.
    async fn work_tx_task(
        work_registry: Arc<Mutex<registry::WorkRegistry>>,
        mining_stats: Arc<Mutex<stats::Mining>>,
        mut tx_fifo: io::WorkTx,
        mut work_generator: work::Generator,
    ) {
        loop {
            await!(tx_fifo.wait_for_room()).expect("wait for tx room");
            let work = await!(work_generator.generate());
            match work {
                None => return,
                Some(work) => {
                    // assign `work_id` to `work`
                    let work_id = await!(work_registry.lock()).store_work(work.clone());
                    // send work is synchronous
                    tx_fifo.send_work(&work, work_id).expect("send work");
                    let mut stats = await!(mining_stats.lock());
                    stats.work_generated += work.midstates.len();
                }
            }
        }
    }

    /// This task receives solutions from hardware, looks up `Assignment` in
    /// registry (under `work_id` got from FPGA), pairs them together and
    /// sends them back to frontend (via `solution_sender`).
    /// If solution is duplicated, it gets dropped (and errors stats incremented).
    /// It prints warnings when solution doesn't hit ASIC target.
    /// TODO: this task is not very platform dependent, maybe move it somewhere else?
    /// TODO: figure out when and how to stop this task
    async fn solution_rx_task(
        work_registry: Arc<Mutex<registry::WorkRegistry>>,
        mining_stats: Arc<Mutex<stats::Mining>>,
        mut rx_fifo: io::WorkRx,
        solution_sender: work::SolutionSender,
    ) {
        // solution receiving/filtering part
        loop {
            let (rx_fifo_out, solution) =
                await!(rx_fifo.recv_solution()).expect("recv solution failed");
            rx_fifo = rx_fifo_out;
            let work_id = solution.hardware_id;
            let mut stats = await!(mining_stats.lock());
            let mut work_registry = await!(work_registry.lock());

            let work = work_registry.find_work(work_id as usize);
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

    /// Monitor task
    /// Fetch periodically information about hashrate
    async fn monitor_task(hash_chain: Arc<Mutex<Self>>) {
        info!("monitor task is starting");
        loop {
            await!(sleep(Duration::from_secs(5)));

            let mut hash_chain = await!(hash_chain.lock());
            let responses =
                await!(hash_chain.read_register(ChipAddress::All, bm1387::HASHRATE_REG))
                    .expect("reading hashrate_reg failed");

            if responses.len() != hash_chain.chip_count {
                warn!("missing some responses");
            }
            let mut sum = 0;
            for (chip_address, reg) in responses.iter().enumerate() {
                let hashrate_reg = bm1387::HashrateReg::from_reg(*reg).expect("unpacking failed");
                trace!(
                    "chip {} hashrate {} GHash/s",
                    chip_address,
                    hashrate_reg.hashrate() as f64 / 1e9
                );
                sum += hashrate_reg.hashrate() as u128;
            }
            info!("total chip hashrate {} GHash/s", sum as f64 / 1e9);
        }
    }

    fn spawn_tx_task(
        hash_chain: Arc<Mutex<Self>>,
        work_registry: Arc<Mutex<registry::WorkRegistry>>,
        mining_stats: Arc<Mutex<stats::Mining>>,
        work_generator: work::Generator,
        shutdown: shutdown::Sender,
    ) {
        ii_async_compat::spawn(async move {
            let mut tx_fifo = await!(hash_chain.lock())
                .work_tx_io
                .take()
                .expect("work-tx io missing");

            await!(Self::send_init_work(
                hash_chain.clone(),
                work_registry.clone(),
                &mut tx_fifo
            ));
            await!(Self::work_tx_task(
                work_registry,
                mining_stats,
                tx_fifo,
                work_generator,
            ));
            shutdown.send("no more work from workhub");
        });
    }

    fn spawn_rx_task(
        hash_chain: Arc<Mutex<Self>>,
        work_registry: Arc<Mutex<registry::WorkRegistry>>,
        mining_stats: Arc<Mutex<stats::Mining>>,
        solution_sender: work::SolutionSender,
    ) {
        ii_async_compat::spawn(async move {
            let rx_fifo = await!(hash_chain.lock())
                .work_rx_io
                .take()
                .expect("work-rx io missing");
            await!(Self::solution_rx_task(
                work_registry,
                mining_stats,
                rx_fifo,
                solution_sender,
            ));
        });
    }

    fn spawn_monitor_task(hash_chain: Arc<Mutex<Self>>) {
        ii_async_compat::spawn(async move {
            await!(Self::monitor_task(hash_chain,));
        });
    }

    pub fn start(
        self,
        work_solver: work::Solver,
        mining_stats: Arc<Mutex<stats::Mining>>,
        shutdown: shutdown::Sender,
    ) -> Arc<Mutex<Self>> {
        let work_registry = Arc::new(Mutex::new(registry::WorkRegistry::new(
            self.work_tx_io
                .as_ref()
                .expect("io missing")
                .work_id_count(),
        )));
        let hash_chain = Arc::new(Mutex::new(self));
        let (work_generator, work_solution) = work_solver.split();

        HashChain::spawn_tx_task(
            hash_chain.clone(),
            work_registry.clone(),
            mining_stats.clone(),
            work_generator,
            shutdown.clone(),
        );
        HashChain::spawn_rx_task(
            hash_chain.clone(),
            work_registry.clone(),
            mining_stats.clone(),
            work_solution,
        );
        HashChain::spawn_monitor_task(hash_chain.clone());

        hash_chain
    }
}

async fn start_miner(
    enabled_chains: Vec<usize>,
    work_solver: work::Solver,
    mining_stats: Arc<Mutex<stats::Mining>>,
    shutdown: shutdown::Sender,
    midstate_count: usize,
) {
    let gpio_mgr = gpio::ControlPinManager::new();
    let voltage_ctrl_backend = power::I2cBackend::new(0);
    let voltage_ctrl_backend = power::SharedBackend::new(voltage_ctrl_backend);
    let mut hash_chains = Vec::new();
    info!(
        "Initializing miner, enabled_chains={:?}, midstate_count={}",
        enabled_chains, midstate_count,
    );
    // instantiate hash chains
    for hashboard_idx in enabled_chains.iter() {
        let hash_chain = HashChain::new(
            &gpio_mgr,
            voltage_ctrl_backend.clone(),
            *hashboard_idx,
            MidstateCount::new(midstate_count),
            config::ASIC_DIFFICULTY,
        )
        .unwrap();
        hash_chains.push(hash_chain);
    }
    // initialize hash chains (sequentially)
    for hash_chain in hash_chains.iter_mut() {
        await!(hash_chain.init()).expect("miner initialization failed");
    }
    // spawn worker tasks for each hash chain and start mining
    for hash_chain in hash_chains.drain(..) {
        hash_chain.start(work_solver.clone(), mining_stats.clone(), shutdown.clone());
    }
}

/// Represents raw solution from the Antminer S9
#[derive(Clone, Debug)]
pub struct Solution {
    /// Actual nonce
    nonce: u32,
    /// Index of a midstate that corresponds to the found nonce
    midstate_idx: usize,
    /// Index of a solution (if multiple were found)
    solution_idx: usize,
    /// Hardware specific solution identifier
    pub hardware_id: u32,
}

impl hal::BackendSolution for Solution {
    #[inline]
    fn nonce(&self) -> u32 {
        self.nonce
    }

    #[inline]
    fn midstate_idx(&self) -> usize {
        self.midstate_idx
    }

    #[inline]
    fn solution_idx(&self) -> usize {
        self.solution_idx
    }
}

pub struct Backend;

impl hal::Backend for Backend {
    const DEFAULT_MIDSTATE_COUNT: usize = config::DEFAULT_MIDSTATE_COUNT;
    const JOB_TIMEOUT: Duration = config::JOB_TIMEOUT;

    /// Starts statistics tasks specific for S9
    fn start_mining_stats_task(mining_stats: Arc<Mutex<stats::Mining>>) {
        ii_async_compat::spawn(stats::hashrate_meter_task_hashchain(mining_stats));
        ii_async_compat::spawn(stats::hashrate_meter_task());
    }

    fn run(
        &self,
        work_solver: work::Solver,
        mining_stats: Arc<Mutex<stats::Mining>>,
        shutdown: shutdown::Sender,
    ) {
        ii_async_compat::spawn(start_miner(
            vec![config::S9_HASHBOARD_INDEX],
            work_solver,
            mining_stats,
            shutdown,
            runtime_config::get_midstate_count(),
        ));
    }
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
fn calculate_work_delay_for_pll(n_midstates: usize, pll_frequency: usize) -> f64 {
    let space_size_per_core: u64 = 1 << 19;
    0.9 * (n_midstates as u64 * space_size_per_core) as f64 / pll_frequency as f64
}

/// Helper method to convert seconds to FPGA ticks suitable to be written
/// to `WORK_TIME` FPGA register.
///
/// Returns number of ticks.
fn secs_to_fpga_ticks(secs: f64) -> u32 {
    (secs * FPGA_IPCORE_F_CLK_SPEED_HZ as f64) as u32
}
