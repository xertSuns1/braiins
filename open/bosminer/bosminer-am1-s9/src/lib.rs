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
#![recursion_limit = "256"]

mod async_i2c;
pub mod bm1387;
mod cgminer;
pub mod command;
pub mod config;
pub mod counters;
pub mod error;
pub mod fan;
pub mod gpio;
pub mod halt;
pub mod hooks;
pub mod i2c;
pub mod io;
pub mod monitor;
pub mod null_work;
pub mod power;
pub mod registry;
pub mod sensor;
pub mod utils;

#[cfg(test)]
pub mod test;

use ii_logging::macros::*;

use bosminer::async_trait;
use bosminer::hal::{self, BackendConfig as _};
use bosminer::node;
use bosminer::stats;
use bosminer::work;

use bosminer_macros::WorkSolverNode;

use std::fmt;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use error::ErrorKind;
use failure::ResultExt;

use futures::channel::mpsc;
use futures::lock::Mutex;
use futures::stream::StreamExt;
use ii_async_compat::futures;

use bm1387::{ChipAddress, MidstateCount};
use command::Interface;

use packed_struct::PackedStruct;

use embedded_hal::digital::v2::InputPin;
use embedded_hal::digital::v2::OutputPin;

use ii_bitcoin::MeetsTarget;

use ii_async_compat::tokio;
use tokio::sync::watch;
use tokio::time::delay_for;

/// Timing constants
const INACTIVATE_FROM_CHAIN_DELAY: Duration = Duration::from_millis(100);
/// Base delay quantum during hashboard initialization
const INIT_DELAY: Duration = Duration::from_secs(1);
/// Time to wait between successive hashboard initialization attempts
const ENUM_RETRY_DELAY: Duration = Duration::from_secs(10);
/// How many times to retry the enumeration
const ENUM_RETRY_COUNT: usize = 10;

/// Maximum number of chips is limitted by the fact that there is only 8-bit address field and
/// addresses to the chips need to be assigned with step of 4 (e.g. 0, 4, 8, etc.)
pub const MAX_CHIPS_ON_CHAIN: usize = 64;
/// Number of chips to consider OK for initialization
pub const EXPECTED_CHIPS_ON_CHAIN: usize = 63;

/// Oscillator speed for all chips on S9 hash boards
pub const CHIP_OSC_CLK_HZ: usize = 25_000_000;

/// Exact value of the initial baud rate after reset of the hashing chips.
const INIT_CHIP_BAUD_RATE: usize = 115740;
/// Exact desired target baud rate when hashing at full speed (matches the divisor, too)
const TARGET_CHIP_BAUD_RATE: usize = 1562500;

/// Address of chip with connected temp sensor
const TEMP_CHIP: ChipAddress = ChipAddress::One(61);

/// Timeout for completion of haschain halt
const HALT_TIMEOUT: Duration = Duration::from_secs(30);

/// Core address space size (it should be 114, but the addresses are non-consecutive)
const CORE_ADR_SPACE_SIZE: usize = 128;

/// Power type alias
/// TODO: Implement it as a proper type (not just alias)
pub type Power = usize;

/// Type representing plug pin
#[derive(Clone)]
pub struct PlugPin {
    pin: gpio::PinIn,
}

impl PlugPin {
    pub fn open(gpio_mgr: &gpio::ControlPinManager, hashboard_idx: usize) -> error::Result<Self> {
        Ok(Self {
            pin: gpio_mgr
                .get_pin_in(gpio::PinInName::Plug(hashboard_idx))
                .context(ErrorKind::Hashboard(
                    hashboard_idx,
                    "failed to initialize plug pin".to_string(),
                ))?,
        })
    }

    pub fn hashboard_present(&self) -> error::Result<bool> {
        Ok(self.pin.is_high()?)
    }
}

/// Type representing reset pin
#[derive(Clone)]
pub struct ResetPin {
    pin: gpio::PinOut,
}

impl ResetPin {
    pub fn open(gpio_mgr: &gpio::ControlPinManager, hashboard_idx: usize) -> error::Result<Self> {
        Ok(Self {
            pin: gpio_mgr
                .get_pin_out(gpio::PinOutName::Rst(hashboard_idx))
                .context(ErrorKind::Hashboard(
                    hashboard_idx,
                    "failed to initialize reset pin".to_string(),
                ))?,
        })
    }

    pub fn enter_reset(&mut self) -> error::Result<()> {
        self.pin.set_low()?;
        Ok(())
    }

    pub fn exit_reset(&mut self) -> error::Result<()> {
        self.pin.set_high()?;
        Ok(())
    }
}

/// Hash Chain Controller provides abstraction of the FPGA interface for operating hashing boards.
/// It is the user-space driver for the IP Core
///
/// Main responsibilities:
/// - memory mapping of the FPGA control interface
/// - mining work submission and solution processing
///
/// TODO: disable voltage controller via async `Drop` trait (which doesn't exist yet)
pub struct HashChain {
    /// Number of chips that have been detected
    chip_count: usize,
    /// Eliminates the need to query the IP core about the current number of configured midstates
    midstate_count: MidstateCount,
    /// ASIC difficulty
    asic_difficulty: usize,
    /// ASIC target (matches difficulty)
    asic_target: ii_bitcoin::Target,
    /// Voltage controller on this hashboard
    voltage_ctrl: Arc<power::Control>,
    /// Pin for resetting the hashboard
    reset_pin: ResetPin,
    hashboard_idx: usize,
    pub command_context: command::Context,
    pub common_io: io::Common,
    work_rx_io: Mutex<Option<io::WorkRx>>,
    work_tx_io: Mutex<Option<io::WorkTx>>,
    monitor_tx: mpsc::UnboundedSender<monitor::Message>,
    /// Do not send open-core work if this is true (some tests that test chip initialization may
    /// want to do this).
    disable_init_work: bool,
    /// channels through which temperature status is sent
    temperature_sender: Mutex<Option<watch::Sender<Option<sensor::Temperature>>>>,
    temperature_receiver: watch::Receiver<Option<sensor::Temperature>>,
    /// nonce counter
    pub counter: Arc<Mutex<counters::HashChain>>,
    /// halter to stop this hashchain
    halt_sender: Arc<halt::Sender>,
    /// we need to keep the halt receiver around, otherwise the "stop-notify" channel closes when chain ends
    #[allow(dead_code)]
    halt_receiver: halt::Receiver,
    /// Current hashchain settings
    frequency: Mutex<FrequencySettings>,
}

impl HashChain {
    /// Creates a new hashboard controller with memory mapped FPGA IP core
    ///
    /// * `gpio_mgr` - gpio manager used for producing pins required for hashboard control
    /// * `voltage_ctrl_backend` - communication backend for the voltage controller
    /// * `hashboard_idx` - index of this hashboard determines which FPGA IP core is to be mapped
    /// * `midstate_count` - see Self
    /// * `asic_difficulty` - to what difficulty set the hardware target filter
    pub fn new(
        reset_pin: ResetPin,
        plug_pin: PlugPin,
        voltage_ctrl_backend: Arc<power::I2cBackend>,
        hashboard_idx: usize,
        midstate_count: MidstateCount,
        asic_difficulty: usize,
        monitor_tx: mpsc::UnboundedSender<monitor::Message>,
    ) -> error::Result<Self> {
        let core = io::Core::new(hashboard_idx, midstate_count)?;
        // Unfortunately, we have to do IP core re-init here (but it should be OK, it's synchronous)
        let (common_io, command_io, work_rx_io, work_tx_io) = core.init_and_split()?;

        // check that the board is present
        if !plug_pin.hashboard_present()? {
            Err(ErrorKind::Hashboard(
                hashboard_idx,
                "not present".to_string(),
            ))?
        }

        // create temperature sending channel
        let (temperature_sender, temperature_receiver) = watch::channel(None);

        // create halt notification channel
        let (halt_sender, halt_receiver) = halt::make_pair(HALT_TIMEOUT);

        Ok(Self {
            chip_count: 0,
            midstate_count,
            asic_difficulty,
            asic_target: ii_bitcoin::Target::from_pool_difficulty(asic_difficulty),
            voltage_ctrl: Arc::new(power::Control::new(voltage_ctrl_backend, hashboard_idx)),
            reset_pin,
            hashboard_idx,
            common_io,
            command_context: command::Context::new(command_io),
            work_rx_io: Mutex::new(Some(work_rx_io)),
            work_tx_io: Mutex::new(Some(work_tx_io)),
            monitor_tx,
            disable_init_work: false,
            temperature_sender: Mutex::new(Some(temperature_sender)),
            temperature_receiver,
            counter: Arc::new(Mutex::new(counters::HashChain::new(MAX_CHIPS_ON_CHAIN))),
            halt_sender,
            halt_receiver,
            frequency: Mutex::new(FrequencySettings::from_frequency(0)),
        })
    }

    pub fn current_temperature(&self) -> Option<sensor::Temperature> {
        self.temperature_receiver.borrow().clone()
    }

    async fn take_work_rx_io(&self) -> io::WorkRx {
        self.work_rx_io
            .lock()
            .await
            .take()
            .expect("work-rx io missing")
    }

    async fn take_work_tx_io(&self) -> io::WorkTx {
        self.work_tx_io
            .lock()
            .await
            .take()
            .expect("work-tx io missing")
    }

    /// Calculate work_time for this instance of HChain
    ///
    /// Returns number of ticks (suitable to be written to `WORK_TIME` register)
    #[inline]
    fn calculate_work_time(&self, max_pll_frequency: usize) -> u32 {
        secs_to_fpga_ticks(calculate_work_delay_for_pll(
            self.midstate_count.to_count(),
            max_pll_frequency,
        ))
    }

    /// Set work time depending on current PLL frequency
    ///
    /// This method sets work time so it's fast enough for `new_freq`
    async fn set_work_time(&self, new_freq: usize) {
        let new_work_time = self.calculate_work_time(new_freq);
        info!("Using work time: {} for freq {}", new_work_time, new_freq);
        self.common_io.set_ip_core_work_time(new_work_time);
    }

    /// Helper method that initializes the FPGA IP core
    async fn ip_core_init(&mut self) -> error::Result<()> {
        // Configure IP core
        self.set_ip_core_baud_rate(INIT_CHIP_BAUD_RATE)?;
        self.common_io.set_midstate_count();

        Ok(())
    }

    /// Puts the board into reset mode and disables the associated IP core
    fn enter_reset(&mut self) -> error::Result<()> {
        self.common_io.disable_ip_core();
        // Warning: Reset pin DOESN'T reset the PIC. The PIC needs to be reset by other means.
        // Perform reset of the hashboard
        self.reset_pin.enter_reset()?;
        Ok(())
    }

    /// Leaves reset mode
    fn exit_reset(&mut self) -> error::Result<()> {
        self.reset_pin.exit_reset()?;
        self.common_io.enable_ip_core();
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
        self.command_context
            .write_register_readback(ChipAddress::All, &tm_reg)
            .await?;
        Ok(())
    }

    /// Reset hashboard and try to enumerate the chips.
    /// If not enough chips were found and `accept_less_chips` is not specified,
    /// treat it as error.
    async fn reset_and_enumerate_and_init(
        &mut self,
        accept_less_chips: bool,
        initial_frequency: &FrequencySettings,
    ) -> error::Result<()> {
        // Reset hashboard, toggle voltage
        info!("Resetting hash board");
        self.enter_reset()?;
        self.voltage_ctrl.disable_voltage().await?;
        delay_for(INIT_DELAY).await;
        self.voltage_ctrl.enable_voltage().await?;
        delay_for(INIT_DELAY * 2).await;
        self.exit_reset()?;
        delay_for(INIT_DELAY).await;

        // Enumerate chips
        info!("Starting chip enumeration");
        self.enumerate_chips().await?;

        // Figure out if we found enough chips
        info!("Discovered {} chips", self.chip_count);
        self.command_context.set_chip_count(self.chip_count).await;
        self.counter.lock().await.set_chip_count(self.chip_count);
        self.frequency.lock().await.set_chip_count(self.chip_count);

        // If we don't have full number of chips and we do not want incomplete chain, then raise
        // an error
        if self.chip_count < EXPECTED_CHIPS_ON_CHAIN && !accept_less_chips {
            Err(ErrorKind::ChipEnumeration(
                "Not enough chips on chain".into(),
            ))?;
        }

        // set PLL
        self.set_pll(initial_frequency).await?;

        // configure the hashing chain to operate at desired baud rate. Note that gate block is
        // enabled to allow continuous start of chips in the chain
        self.configure_hash_chain(TARGET_CHIP_BAUD_RATE, false, true)
            .await?;
        self.set_ip_core_baud_rate(TARGET_CHIP_BAUD_RATE)?;

        self.set_asic_diff(self.asic_difficulty).await?;

        Ok(())
    }

    /// Initializes the complete hashboard including enumerating all chips
    ///
    /// * if enumeration fails (for enumeration-related reason), try to retry
    ///   it up to pre-defined number of times
    /// * if less than 63 chips is found, retry the enumeration
    async fn init(
        &mut self,
        initial_frequency: &FrequencySettings,
        initial_voltage: power::Voltage,
        accept_less_chips: bool,
    ) -> error::Result<Arc<Mutex<registry::WorkRegistry>>> {
        info!("Hashboard IP core initialized");
        self.voltage_ctrl
            .clone()
            .init(self.halt_receiver.clone())
            .await?;

        info!(
            "Initializing hash chain {}, (difficulty {})",
            self.hashboard_idx, self.asic_difficulty
        );
        self.ip_core_init().await?;

        // Enumerate chips
        self.reset_and_enumerate_and_init(accept_less_chips, initial_frequency)
            .await?;

        // Build shared work registry
        // TX fifo determines the size of work registry
        let work_registry = Arc::new(Mutex::new(registry::WorkRegistry::new(
            self.work_tx_io
                .lock()
                .await
                .as_ref()
                .expect("work-tx io missing")
                .work_id_count(),
        )));

        // send opencore work (at high voltage) unless someone disabled it
        if !self.disable_init_work {
            self.send_init_work(work_registry.clone()).await;
        }

        // lower voltage to working level
        self.voltage_ctrl
            .set_voltage(initial_voltage)
            .await
            .expect("lowering voltage failed");

        // return work registry we created
        Ok(work_registry)
    }

    /// Detects the number of chips on the hashing chain and assigns an address to each chip
    async fn enumerate_chips(&mut self) -> error::Result<()> {
        // Enumerate all chips (broadcast read address register request)
        let responses = self
            .command_context
            .read_register::<bm1387::GetAddressReg>(ChipAddress::All)
            .await?;

        // Reset chip count (we might get called multiple times)
        self.chip_count = 0;
        // Check if are responses meaningful
        for (address, addr_reg) in responses.iter().enumerate() {
            if addr_reg.chip_rev != bm1387::CHIP_REV_BM1387 {
                Err(ErrorKind::ChipEnumeration(format!(
                    "unexpected revision of chip {} (expected: {:#x?} received: {:#x?})",
                    address,
                    bm1387::CHIP_REV_BM1387,
                    addr_reg.chip_rev,
                )))?
            }
            self.chip_count += 1;
        }
        if self.chip_count >= MAX_CHIPS_ON_CHAIN {
            Err(ErrorKind::ChipEnumeration(format!(
                "detected {} chips, expected less than {} chips on one chain. Possibly a hardware issue?",
                self.chip_count,
                MAX_CHIPS_ON_CHAIN,
            )))?
        }
        if self.chip_count == 0 {
            Err(ErrorKind::ChipEnumeration(
                "no chips detected on the current chain".to_string(),
            ))?
        }

        // Set all chips to be offline before address assignment. This is important so that each
        // chip after initially accepting the address will pass on further addresses down the chain
        let inactivate_from_chain_cmd = bm1387::InactivateFromChainCmd::new().pack();
        // make sure all chips receive inactivation request
        for _ in 0..3 {
            self.command_context
                .send_raw_command(inactivate_from_chain_cmd.to_vec(), false)
                .await;
            delay_for(INACTIVATE_FROM_CHAIN_DELAY).await;
        }

        // Assign address to each chip
        for i in 0..self.chip_count {
            let cmd = bm1387::SetChipAddressCmd::new(ChipAddress::One(i));
            self.command_context
                .send_raw_command(cmd.pack().to_vec(), false)
                .await;
        }

        Ok(())
    }

    /// Loads PLL register with a starting value
    ///
    /// WARNING: you have to take care of `set_work_time` yourself
    async fn set_chip_pll(&self, chip_addr: ChipAddress, freq: usize) -> error::Result<()> {
        // convert frequency to PLL setting register
        let pll = bm1387::PllFrequency::lookup_freq(freq)?;

        info!(
            "chain {}: setting frequency {} MHz on {:?} (error {} MHz)",
            self.hashboard_idx,
            freq / 1_000_000,
            chip_addr,
            ((freq as f64) - (pll.frequency as f64)).abs() / 1_000_000.0,
        );

        // NOTE: When PLL register is read back, it is or-ed with 0x8000_0000, not sure why.
        //  Avoid reading it back to prevent disappointment.
        self.command_context
            .write_register(chip_addr, &pll.reg)
            .await?;

        Ok(())
    }

    /// Load PLL register of all chips
    ///
    /// Takes care of adjusting `work_time`
    pub async fn set_pll(&self, frequency: &FrequencySettings) -> error::Result<()> {
        // TODO: find a better way - how to communicate with frequency setter how many chips we have?
        assert!(frequency.chip.len() >= self.chip_count);

        // Check if the frequencies are identical
        if frequency.min() == frequency.max() {
            // Update them in one go
            self.set_chip_pll(ChipAddress::All, frequency.chip[0])
                .await?;
        } else {
            // Update chips one-by-one
            for i in 0..self.chip_count {
                let new_freq = self.frequency.lock().await.chip[i];
                if new_freq != frequency.chip[i] {
                    self.set_chip_pll(ChipAddress::One(i), new_freq).await?;
                }
            }
        }

        // Update worktime
        self.set_work_time(frequency.max()).await;

        // Remember what frequencies are set
        let mut cur_frequency = self.frequency.lock().await;
        for i in 0..self.chip_count {
            cur_frequency.chip[i] = frequency.chip[i];
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
        // Do not read back the MiscCtrl register when setting baud rate: it will result
        // in serial speed mismatch and nothing being read.
        self.command_context
            .write_register(ChipAddress::All, &ctl_reg)
            .await?;
        Ok(actual_baud_rate)
    }

    /// This method only changes the communication speed of the FPGA IP core with the chips.
    ///
    /// Note: change baud rate of the FPGA is only desirable as a step after all chips in the
    /// chain have been reconfigured for a different speed, too.
    fn set_ip_core_baud_rate(&self, baud: usize) -> error::Result<()> {
        let (baud_clock_div, actual_baud_rate) =
            calc_baud_clock_div(baud, io::F_CLK_SPEED_HZ, io::F_CLK_BASE_BAUD_DIV)?;
        info!(
            "Setting IP core baud rate @ requested: {}, actual: {}, divisor {:#04x}",
            baud, actual_baud_rate, baud_clock_div
        );

        self.common_io.set_baud_clock_div(baud_clock_div as u32);
        Ok(())
    }

    pub fn get_chip_count(&self) -> usize {
        self.chip_count
    }

    /// Initialize cores by sending open-core work with correct nbits to each core
    async fn send_init_work(&mut self, work_registry: Arc<Mutex<registry::WorkRegistry>>) {
        // Each core gets one work
        const NUM_WORK: usize = bm1387::NUM_CORES_ON_CHIP;
        trace!(
            "Sending out {} pieces of dummy work to initialize chips",
            NUM_WORK
        );
        let midstate_count = self.midstate_count.to_count();
        let mut work_tx_io = self.work_tx_io.lock().await;
        let tx_fifo = work_tx_io.as_mut().expect("tx fifo missing");
        for _ in 0..NUM_WORK {
            let work = &null_work::prepare_opencore(true, midstate_count);
            // store work to registry as "initial work" so that later we can properly ignore
            // solutions
            let work_id = work_registry.lock().await.store_work(work.clone(), true);
            tx_fifo.wait_for_room().await.expect("wait for tx room");
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
        mut tx_fifo: io::WorkTx,
        mut work_generator: work::Generator,
    ) {
        loop {
            tx_fifo.wait_for_room().await.expect("wait for tx room");
            let work = work_generator.generate().await;
            match work {
                None => return,
                Some(work) => {
                    // assign `work_id` to `work`
                    let work_id = work_registry.lock().await.store_work(work.clone(), false);
                    // send work is synchronous
                    tx_fifo.send_work(&work, work_id).expect("send work");
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
        self: Arc<Self>,
        work_registry: Arc<Mutex<registry::WorkRegistry>>,
        mut rx_fifo: io::WorkRx,
        solution_sender: work::SolutionSender,
        counter: Arc<Mutex<counters::HashChain>>,
    ) {
        // solution receiving/filtering part
        loop {
            let (rx_fifo_out, hw_solution) =
                rx_fifo.recv_solution().await.expect("recv solution failed");
            rx_fifo = rx_fifo_out;
            let work_id = hw_solution.hardware_id;
            let solution = Solution::from_hw_solution(&hw_solution, self.asic_target);
            let mut work_registry = work_registry.lock().await;

            let work = work_registry.find_work(work_id as usize);
            match work {
                Some(work_item) => {
                    // ignore solutions coming from initial work
                    if work_item.initial_work {
                        continue;
                    }
                    let core_addr = bm1387::CoreAddress::new(solution.nonce);
                    let status = work_item.insert_solution(solution);

                    // work item detected a new unique solution, we will push it for further processing
                    if let Some(unique_solution) = status.unique_solution {
                        if !status.duplicate {
                            if !unique_solution
                                .hash()
                                .meets(unique_solution.backend_target())
                            {
                                info!("Solution from hashchain not hitting ASIC target");
                                counter.lock().await.add_error(core_addr);
                            } else {
                                counter.lock().await.add_valid(core_addr);
                            }
                            solution_sender.send(unique_solution);
                        }
                    }
                    if status.duplicate {
                        counter.lock().await.add_error(core_addr);
                    }
                    if status.mismatched_nonce {
                        counter.lock().await.add_error(core_addr);
                    }
                }
                None => {
                    info!(
                        "No work present for solution, ID:{:#x} {:#010x?}",
                        work_id, solution
                    );
                }
            }
        }
    }

    async fn try_to_initialize_sensor(
        command_context: command::Context,
    ) -> error::Result<Box<dyn sensor::Sensor>> {
        // construct I2C bus via command interface
        let i2c_bus = bm1387::i2c::Bus::new_and_init(command_context, TEMP_CHIP)
            .await
            .with_context(|_| ErrorKind::Sensors("bus construction failed".into()))?;

        // try to probe sensor
        let sensor = sensor::probe_i2c_sensors(i2c_bus)
            .await
            .with_context(|_| ErrorKind::Sensors("error when probing sensors".into()))?;

        // did we find anything?
        let mut sensor = match sensor {
            Some(sensor) => sensor,
            None => Err(ErrorKind::Sensors("no sensors found".into()))?,
        };

        // try to initialize sensor
        sensor
            .init()
            .await
            .with_context(|_| ErrorKind::Sensors("failed to initialize sensors".into()))?;

        // done
        Ok(sensor)
    }

    /// Monitor watchdog task.
    /// This task sends periodically ping to monitor task. It also tries to read temperature.
    async fn monitor_watchdog_temp_task(self: Arc<Self>) {
        // fetch hashboard idx
        info!(
            "Monitor watchdog temperature task started for hashchain {}",
            self.hashboard_idx
        );

        // take out temperature sender channel
        let temperature_sender = self
            .temperature_sender
            .lock()
            .await
            .take()
            .expect("BUG: temperature sender missing");

        // Wait some time before trying to initialize temperature controller
        // (Otherwise RX queue might be clogged with initial work and we will not get any replies)
        //
        // TODO: we should implement a more robust mechanism that controls access to the I2C bus of
        // a hashing chip only if the hashchain allows it (hashchain is in operation etc.)
        delay_for(Duration::from_secs(5)).await;

        // Try to probe sensor
        // This may fail - in which case we put `None` into `sensor`
        let mut sensor = match Self::try_to_initialize_sensor(self.command_context.clone())
            .await
            .with_context(|_| ErrorKind::Hashboard(self.hashboard_idx, "sensor error".into()))
            .map_err(|e| e.into())
        {
            error::Result::Err(e) => {
                error!("Sensor probing failed: {}", e);
                None
            }
            error::Result::Ok(sensor) => Some(sensor),
        };

        // "Watchdog" loop that pings monitor every some seconds
        loop {
            // If we have temperature sensor, try to read it
            let temp = if let Some(sensor) = sensor.as_mut() {
                match sensor
                    .read_temperature()
                    .await
                    .with_context(|_| {
                        ErrorKind::Hashboard(self.hashboard_idx, "temperature read fail".into())
                    })
                    .map_err(|e| e.into())
                {
                    error::Result::Ok(temp) => {
                        info!("Measured temperature: {:?}", temp);
                        temp
                    }
                    error::Result::Err(e) => {
                        error!("Sensor temperature read failed: {}", e);
                        sensor::INVALID_TEMPERATURE_READING
                    }
                }
            } else {
                // Otherwise just make empty temperature reading
                sensor::INVALID_TEMPERATURE_READING
            };

            // Broadcast
            temperature_sender
                .broadcast(Some(temp.clone()))
                .expect("temp broadcast failed");

            // Send heartbeat to monitor
            self.monitor_tx
                .unbounded_send(monitor::Message::Running(temp))
                .expect("send failed");

            // TODO: sync this delay with monitor task
            delay_for(Duration::from_secs(5)).await;
        }
    }

    /// Hashrate monitor task
    /// Fetch perodically information about hashrate
    #[allow(dead_code)]
    async fn hashrate_monitor_task(self: Arc<Self>) {
        info!("Hashrate monitor task started");
        loop {
            delay_for(Duration::from_secs(5)).await;

            let responses = self
                .command_context
                .read_register::<bm1387::HashrateReg>(ChipAddress::All)
                .await
                .expect("reading hashrate_reg failed");

            let mut sum = 0;
            for (chip_address, hashrate_reg) in responses.iter().enumerate() {
                trace!(
                    "chip {} hashrate {} GHash/s",
                    chip_address,
                    hashrate_reg.hashrate() as f64 / 1e9
                );
                sum += hashrate_reg.hashrate() as u128;
            }
            info!("Total chip hashrate {} GH/s", sum as f64 / 1e9);
        }
    }

    async fn start(
        self: Arc<Self>,
        work_generator: work::Generator,
        solution_sender: work::SolutionSender,
        work_registry: Arc<Mutex<registry::WorkRegistry>>,
    ) {
        // spawn tx task
        let tx_fifo = self.take_work_tx_io().await;
        self.halt_receiver
            .register_client("work-tx".into())
            .await
            .spawn(Self::work_tx_task(
                work_registry.clone(),
                tx_fifo,
                work_generator,
            ));

        // spawn rx task
        let rx_fifo = self.take_work_rx_io().await;
        self.halt_receiver
            .register_client("work-rx".into())
            .await
            .spawn(Self::solution_rx_task(
                self.clone(),
                work_registry.clone(),
                rx_fifo,
                solution_sender,
                self.counter.clone(),
            ));

        // spawn hashrate monitor
        // Disabled until we found a use for this
        /*
        self.halt_receiver
            .register_client("hashrate monitor".into())
            .await
            .spawn(Self::hashrate_monitor_task(self.clone()));
        */

        // spawn temperature monitor
        self.halt_receiver
            .register_client("temperature monitor".into())
            .await
            .spawn(Self::monitor_watchdog_temp_task(self.clone()));
    }

    pub async fn reset_counter(&self) {
        self.counter.lock().await.reset();
    }

    pub async fn snapshot_counter(&self) -> counters::HashChain {
        self.counter.lock().await.snapshot()
    }

    pub async fn get_frequency(&self) -> FrequencySettings {
        self.frequency.lock().await.clone()
    }

    pub async fn get_voltage(&self) -> power::Voltage {
        self.voltage_ctrl
            .get_current_voltage()
            .await
            .expect("BUG: no voltage on hashchain")
    }
}

impl fmt::Debug for HashChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash Board {}", self.hashboard_idx)
    }
}

impl fmt::Display for HashChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash Board {}", self.hashboard_idx)
    }
}

type Frequency = usize;

#[derive(Clone)]
pub struct FrequencySettings {
    pub chip: Vec<Frequency>,
}

impl FrequencySettings {
    /// Build frequency settings with all chips having the same frequency
    pub fn from_frequency(frequency: usize) -> Self {
        Self {
            chip: vec![frequency; EXPECTED_CHIPS_ON_CHAIN],
        }
    }

    pub fn set_chip_count(&mut self, chip_count: usize) {
        assert!(self.chip.len() >= chip_count);
        self.chip.resize(chip_count, 0);
    }

    pub fn total(&self) -> u64 {
        self.chip.iter().fold(0, |total_f, &f| total_f + f as u64)
    }

    #[allow(dead_code)]
    pub fn min(&self) -> usize {
        *self.chip.iter().min().expect("BUG: no chips on chain")
    }

    #[allow(dead_code)]
    pub fn max(&self) -> usize {
        *self.chip.iter().max().expect("BUG: no chips on chain")
    }

    pub fn avg(&self) -> usize {
        assert!(self.chip.len() > 0, "BUG: no chips on chain");
        let sum: u64 = self.chip.iter().map(|frequency| *frequency as u64).sum();
        (sum / self.chip.len() as u64) as usize
    }

    fn pretty_frequency(freq: usize) -> String {
        format!("{:.01} MHz", (freq as f32) / 1_000_000.0)
    }
}

impl fmt::Display for FrequencySettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let min = self.min();
        let max = self.max();
        if min == max {
            write!(f, "{} (all chips)", Self::pretty_frequency(min))
        } else {
            write!(
                f,
                "{} (min {}, max {})",
                Self::pretty_frequency((self.total() / (self.chip.len() as u64)) as Frequency),
                Self::pretty_frequency(min),
                Self::pretty_frequency(max)
            )
        }
    }
}

#[derive(Debug)]
pub struct StoppedChain {
    pub manager: Arc<Manager>,
}

impl Drop for StoppedChain {
    fn drop(&mut self) {
        // remove ownership in case we are dropped
        self.manager
            .owned_by
            .lock()
            .expect("BUG: lock failed")
            .take();
    }
}

impl StoppedChain {
    pub async fn start(
        self,
        initial_frequency: &FrequencySettings,
        initial_voltage: power::Voltage,
    ) -> Result<RunningChain, (Self, error::Error)> {
        // if miner initialization fails, retry
        let mut tries_left = ENUM_RETRY_COUNT;

        loop {
            info!(
                "Registering hashboard {} with monitor",
                self.manager.hashboard_idx
            );

            // Start this hashchain
            // If we've already exhausted half of our tries, then stop worrying about having
            // less chips than expected (63).
            match self
                .manager
                .attempt_start_chain(
                    tries_left <= ENUM_RETRY_COUNT / 2,
                    initial_frequency,
                    initial_voltage,
                )
                .await
            {
                // start successful
                Ok(_) => {
                    // we've started the hashchain
                    // create a `Running` tape and be gone
                    return Ok(RunningChain {
                        manager: self.manager.clone(),
                        start_id: self.manager.inner.lock().await.start_count,
                    });
                }
                // start failed
                Err(e) => {
                    error!("Chain {} start failed: {}", self.manager.hashboard_idx, e);

                    // retry if possible
                    if tries_left == 0 {
                        error!("No tries left");
                        return Err((self, e.into()));
                    } else {
                        tries_left -= 1;
                        // TODO: wait with locks unlocked()! Otherwise no-one can halt the miner
                        // This is not possible with current lock design, but fix this ASAP!
                        delay_for(ENUM_RETRY_DELAY).await;
                        info!("Retrying chain {} start...", self.manager.hashboard_idx);
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct RunningChain {
    pub manager: Arc<Manager>,
    pub start_id: usize,
}

impl Drop for RunningChain {
    fn drop(&mut self) {
        // remove ownership in case we are dropped
        self.manager
            .owned_by
            .lock()
            .expect("BUG: lock failed")
            .take();
    }
}

impl RunningChain {
    pub async fn stop(self) -> StoppedChain {
        self.manager.stop_chain(false).await;

        StoppedChain {
            manager: self.manager.clone(),
        }
    }

    /// TODO: for the love of god use macros or something
    pub async fn get_frequency(&self) -> FrequencySettings {
        let inner = self.manager.inner.lock().await;
        inner
            .hash_chain
            .as_ref()
            .expect("BUG: hashchain is not running")
            .get_frequency()
            .await
    }

    /// TODO: for the love of god use macros or something
    pub async fn get_voltage(&self) -> power::Voltage {
        let inner = self.manager.inner.lock().await;
        inner
            .hash_chain
            .as_ref()
            .expect("BUG: hashchain is not running")
            .get_voltage()
            .await
    }

    pub async fn set_frequency(&self, frequency: &FrequencySettings) -> error::Result<()> {
        let inner = self.manager.inner.lock().await;
        inner
            .hash_chain
            .as_ref()
            .expect("BUG: hashchain is not running")
            .set_pll(frequency)
            .await
    }

    pub async fn set_voltage(&self, voltage: power::Voltage) -> error::Result<()> {
        let inner = self.manager.inner.lock().await;
        inner
            .hash_chain
            .as_ref()
            .expect("BUG: hashchain is not running")
            .voltage_ctrl
            .set_voltage(voltage)
            .await
    }

    pub async fn reset_counter(&self) {
        self.manager
            .inner
            .lock()
            .await
            .hash_chain
            .as_ref()
            .expect("not running")
            .reset_counter()
            .await;
    }

    pub async fn snapshot_counter(&self) -> counters::HashChain {
        self.manager
            .inner
            .lock()
            .await
            .hash_chain
            .as_ref()
            .expect("not running")
            .snapshot_counter()
            .await
    }

    pub async fn current_temperature(&self) -> Option<sensor::Temperature> {
        self.manager
            .inner
            .lock()
            .await
            .hash_chain
            .as_ref()
            .expect("not running")
            .current_temperature()
    }

    /// Check from `Monitor` status message if miner is hot enough
    /// Also: this will break if there are no temperature sensors
    fn preheat_ok(status: monitor::Status) -> bool {
        const PREHEAT_TEMP_EPSILON: f32 = 2.0;
        let target_temp;
        // check if we are in PID mode, otherwise return `true`
        match status.config.fan_config {
            // Can't preheat if we are not controlling fans
            None => return true,
            Some(fan_config) => match fan_config.mode {
                monitor::FanControlMode::TargetTemperature(t) => target_temp = t,
                _ => return true,
            },
        }
        info!(
            "Preheat: waiting for target temperature: {}, current temperature: {:?}",
            target_temp, status.input_temperature
        );
        // we are in PID mode, check if temperature is OK
        match status.input_temperature {
            monitor::ChainTemperature::Ok(t) => {
                if t >= target_temp || target_temp - t < PREHEAT_TEMP_EPSILON {
                    info!("Preheat: temperature {} is hot enough", t);
                    return true;
                }
            }
            _ => (),
        }
        return false;
    }

    /// Wait for hashboard to reach PID-defined temperature (or higher)
    /// If monitor isn't in PID mode then this is effectively no-op.
    /// Wait at most predefined number of seconds to avoid any kind of dead-locks.
    ///
    /// Note: we have to lock it on the inside, because otherwise we would hold lock on hashchain
    /// manager and prevent shutdown from happening.
    pub async fn wait_for_preheat(&self) {
        const MAX_PREHEAT_DELAY: u64 = 180;

        let mut status_receiver = self.manager.status_receiver.clone();
        // wait for status from monitor
        let started = Instant::now();
        // TODO: wrap `status_receiver` into some kind of API
        while let Some(status) = status_receiver.next().await {
            // take just non-empty status messages
            if let Some(status) = status {
                if Self::preheat_ok(status) {
                    break;
                }
            }
            // in case we are waiting for too long, just skip preheat
            if Instant::now().duration_since(started).as_secs() >= MAX_PREHEAT_DELAY {
                info!("Preheat: waiting too long to heat-up, skipping preheat");
                return;
            }
        }
    }
}

pub enum ChainStatus {
    Running(RunningChain),
    Stopped(StoppedChain),
}

impl ChainStatus {
    pub fn expect_stopped(self) -> StoppedChain {
        match self {
            Self::Stopped(s) => s,
            _ => panic!("BUG: expected stopped chain"),
        }
    }
}

pub struct ManagerInner {
    pub hash_chain: Option<Arc<HashChain>>,
    /// Each (attempted) hashchain start increments this counter by 1
    pub start_count: usize,
}

/// Hashchain manager that can start and stop instances of hashchain
/// TODO: split this structure into outer and inner part so that we can
/// deal with locking issues on the inside.
#[derive(WorkSolverNode)]
pub struct Manager {
    #[member_work_solver_stats]
    work_solver_stats: stats::BasicWorkSolver,
    pub hashboard_idx: usize,
    work_generator: work::Generator,
    solution_sender: work::SolutionSender,
    plug_pin: PlugPin,
    reset_pin: ResetPin,
    voltage_ctrl_backend: Arc<power::I2cBackend>,
    midstate_count: MidstateCount,
    asic_difficulty: usize,
    /// channel to report to the monitor
    monitor_tx: mpsc::UnboundedSender<monitor::Message>,
    /// TODO: wrap this type in a structure (in Monitor)
    pub status_receiver: watch::Receiver<Option<monitor::Status>>,
    owned_by: StdMutex<Option<&'static str>>,
    pub inner: Mutex<ManagerInner>,
    pub chain_config: config::ResolvedChainConfig,
}

impl Manager {
    /// Acquire stopped or running chain
    pub async fn acquire(
        self: Arc<Self>,
        owner_name: &'static str,
    ) -> Result<ChainStatus, &'static str> {
        // acquire ownership of the hashchain
        {
            let mut owned_by = self.owned_by.lock().expect("BUG: failed to lock mutex");
            if let Some(already_owned_by) = *owned_by {
                return Err(already_owned_by);
            }
            owned_by.replace(owner_name);
        }
        // Create a `Chain` instance. If it's dropped, the ownership reverts back to `Manager`
        let inner = self.inner.lock().await;
        Ok(if inner.hash_chain.is_some() {
            ChainStatus::Running(RunningChain {
                manager: self.clone(),
                start_id: inner.start_count,
            })
        } else {
            ChainStatus::Stopped(StoppedChain {
                manager: self.clone(),
            })
        })
    }

    /// Initialize and start mining on hashchain
    /// TODO: this function is private and should be called only from `Stopped`
    async fn attempt_start_chain(
        &self,
        accept_less_chips: bool,
        initial_frequency: &FrequencySettings,
        initial_voltage: power::Voltage,
    ) -> error::Result<()> {
        // lock inner to guarantee atomicity of hashchain start
        let mut inner = self.inner.lock().await;

        // register us with monitor
        self.monitor_tx
            .unbounded_send(monitor::Message::On)
            .expect("BUG: send failed");

        // check that we hadn't started some other (?) way
        // TODO: maybe we should throw an error instead
        assert!(inner.hash_chain.is_none());

        // Increment start counter
        inner.start_count += 1;

        // make us a hash chain
        let mut hash_chain = HashChain::new(
            self.reset_pin.clone(),
            self.plug_pin.clone(),
            self.voltage_ctrl_backend.clone(),
            self.hashboard_idx,
            self.midstate_count,
            self.asic_difficulty,
            self.monitor_tx.clone(),
        )
        .expect("BUG: hashchain instantiation failed");

        // initialize it
        let work_registry = match hash_chain
            .init(initial_frequency, initial_voltage, accept_less_chips)
            .await
        {
            Err(e) => {
                // halt is required to stop voltage heart-beat task
                hash_chain.halt_sender.clone().send_halt().await;
                // deregister us
                self.monitor_tx
                    .unbounded_send(monitor::Message::Off)
                    .expect("BUG: send failed");

                return Err(e)?;
            }
            Ok(a) => a,
        };

        // spawn worker tasks for hash chain and start mining
        let hash_chain = Arc::new(hash_chain);
        hash_chain
            .clone()
            .start(
                self.work_generator.clone(),
                self.solution_sender.clone(),
                work_registry,
            )
            .await;

        // remember we started
        inner.hash_chain.replace(hash_chain);

        Ok(())
    }

    /// TODO: this function is private and should be called only from `RunningChain`
    async fn stop_chain(&self, its_ok_if_its_missing: bool) {
        // lock inner to guarantee atomicity of hashchain stop
        let mut inner = self.inner.lock().await;

        // TODO: maybe we should throw an error instead
        let hash_chain = inner.hash_chain.take();
        if hash_chain.is_none() && its_ok_if_its_missing {
            return;
        }
        let hash_chain = hash_chain.expect("BUG: hashchain is missing");

        // stop everything
        hash_chain.halt_sender.clone().send_halt().await;

        // tell monitor we are done
        self.monitor_tx
            .unbounded_send(monitor::Message::Off)
            .expect("BUG: send failed");
    }

    async fn termination_handler(self: Arc<Self>) {
        self.stop_chain(true).await;
    }
}

#[async_trait]
impl node::WorkSolver for Manager {
    fn get_id(&self) -> Option<usize> {
        Some(self.hashboard_idx)
    }

    async fn get_nominal_hashrate(&self) -> Option<ii_bitcoin::HashesUnit> {
        let inner = self.inner.lock().await;
        match inner.hash_chain.as_ref() {
            Some(hash_chain) => {
                let freq_sum = hash_chain.frequency.lock().await.total();
                Some(((freq_sum as u128) * (bm1387::NUM_CORES_ON_CHIP as u128)).into())
            }
            None => None,
        }
    }
}

impl fmt::Debug for Manager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash Chain {}", self.hashboard_idx)
    }
}

impl fmt::Display for Manager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash Chain {}", self.hashboard_idx)
    }
}

/// Represents solution from the hardware combined with difficulty
#[derive(Clone, Debug)]
pub struct Solution {
    /// Actual nonce
    nonce: u32,
    /// Index of a midstate that corresponds to the found nonce
    midstate_idx: usize,
    /// Index of a solution (if multiple were found)
    solution_idx: usize,
    /// Target to which was this solution solved
    target: ii_bitcoin::Target,
}

impl Solution {
    fn from_hw_solution(hw: &io::Solution, target: ii_bitcoin::Target) -> Self {
        Self {
            nonce: hw.nonce,
            midstate_idx: hw.midstate_idx,
            solution_idx: hw.solution_idx,
            target,
        }
    }
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

    #[inline]
    fn target(&self) -> &ii_bitcoin::Target {
        &self.target
    }
}

#[derive(Debug, WorkSolverNode)]
pub struct Backend {
    #[member_work_solver_stats]
    work_solver_stats: stats::BasicWorkSolver,
}

impl Backend {
    pub fn new() -> Self {
        Self {
            work_solver_stats: Default::default(),
        }
    }

    /// Enumerate present hashboards by querying the plug pin
    pub fn detect_hashboards(gpio_mgr: &gpio::ControlPinManager) -> error::Result<Vec<usize>> {
        let mut detected = vec![];
        // TODO: configure this range somewhere
        for hashboard_idx in 1..=8 {
            let plug_pin = PlugPin::open(gpio_mgr, hashboard_idx)?;
            if plug_pin.hashboard_present()? {
                detected.push(hashboard_idx);
            }
        }
        Ok(detected)
    }

    /// Miner termination handler called when app is shutdown.
    /// Just propagate the shutdown to all hashchain managers
    async fn termination_handler(halt_sender: Arc<halt::Sender>) {
        halt_sender.send_halt().await;
    }

    /// Start miner
    /// TODO: maybe think about having a `Result` error value here?
    async fn start_miner(
        gpio_mgr: &gpio::ControlPinManager,
        enabled_chains: Vec<usize>,
        work_hub: work::SolverBuilder<Backend>,
        backend_config: config::Backend,
        app_halt_receiver: halt::Receiver,
        app_halt_sender: Arc<halt::Sender>,
    ) -> (Vec<Arc<Manager>>, Arc<monitor::Monitor>) {
        // Create hooks
        let hooks = match backend_config.hooks.as_ref() {
            Some(hooks) => hooks.clone(),
            None => Arc::new(hooks::NoHooks),
        };

        // Create new termination context and link it to the main (app) termination context
        let (halt_sender, halt_receiver) = halt::make_pair(HALT_TIMEOUT);
        app_halt_receiver
            .register_client("miner termination".into())
            .await
            .spawn_halt_handler(Self::termination_handler(halt_sender.clone()));
        hooks
            .halt_created(
                halt_sender.clone(),
                halt_receiver.clone(),
                app_halt_sender.clone(),
            )
            .await;

        // Start monitor in main (app) termination context
        // Let it shutdown the main context as well
        let monitor_config = backend_config.resolve_monitor_config();
        info!("Resolved monitor backend_config: {:?}", monitor_config);
        let monitor = monitor::Monitor::new_and_start(
            monitor_config,
            app_halt_sender.clone(),
            app_halt_receiver.clone(),
        )
        .await;
        hooks.monitor_started(monitor.clone()).await;

        let voltage_ctrl_backend = Arc::new(power::I2cBackend::new(0));
        let mut managers = Vec::new();
        info!(
            "Initializing miner, enabled_chains={:?}, midstate_count={}",
            enabled_chains,
            backend_config.midstate_count(),
        );
        // build all hash chain managers and register ourselves with frontend
        for hashboard_idx in enabled_chains {
            // register monitor for this haschain
            let monitor_tx = monitor.register_hashchain(hashboard_idx).await;
            // make pins
            let chain_config = backend_config.resolve_chain_config(hashboard_idx);

            let status_receiver = monitor.status_receiver.clone();

            // build hashchain_node for statistics and static parameters
            let manager = work_hub
                .create_work_solver(|work_generator, solution_sender| {
                    Manager {
                        // TODO: create a new substructure of the miner that will hold all gpio and
                        // "physical-insertion" detection data. This structure will be persistent in
                        // between restarts and will enable early notification that there is no hashboard
                        // inserted (instead find out at mining-time).
                        reset_pin: ResetPin::open(&gpio_mgr, hashboard_idx)
                            .expect("failed to make pin"),
                        plug_pin: PlugPin::open(&gpio_mgr, hashboard_idx)
                            .expect("failed to make pin"),
                        voltage_ctrl_backend: voltage_ctrl_backend.clone(),
                        hashboard_idx,
                        midstate_count: chain_config.midstate_count,
                        asic_difficulty: config::ASIC_DIFFICULTY,
                        work_solver_stats: Default::default(),
                        solution_sender,
                        work_generator,
                        monitor_tx,
                        status_receiver,
                        owned_by: StdMutex::new(None),
                        inner: Mutex::new(ManagerInner {
                            hash_chain: None,
                            start_count: 0,
                        }),
                        chain_config,
                    }
                })
                .await;
            managers.push(manager);
        }

        // start everything
        for manager in managers.iter() {
            let halt_receiver = halt_receiver.clone();
            let manager = manager.clone();

            let initial_frequency = manager.chain_config.frequency.clone();
            let initial_voltage = manager.chain_config.voltage;
            let hooks = hooks.clone();

            tokio::spawn(async move {
                // Register handler to stop hashchain when miner is stopped
                halt_receiver
                    .register_client("hashchain".into())
                    .await
                    .spawn_halt_handler(Manager::termination_handler(manager.clone()));

                // Default `NoHooks` starts hashchains right away
                if hooks.can_start_chain(manager.clone()).await {
                    manager
                        .acquire("main")
                        .await
                        .expect("BUG: failed to acquire hashchain")
                        .expect_stopped()
                        .start(&initial_frequency, initial_voltage)
                        .await
                        .expect("BUG: failed to start hashchain");
                }
            });
        }
        (managers, monitor)
    }
}

#[async_trait]
impl hal::Backend for Backend {
    type Type = Self;
    type Config = config::Backend;

    const DEFAULT_HASHRATE_INTERVAL: Duration = config::DEFAULT_HASHRATE_INTERVAL;
    const JOB_TIMEOUT: Duration = config::JOB_TIMEOUT;

    fn create(_backend_config: &mut config::Backend) -> hal::WorkNode<Self> {
        node::WorkSolverType::WorkHub(Box::new(Self::new))
    }

    async fn init_work_hub(
        backend_config: config::Backend,
        work_hub: work::SolverBuilder<Self>,
    ) -> bosminer::Result<hal::FrontendConfig> {
        let backend = work_hub.to_node().clone();
        let gpio_mgr = gpio::ControlPinManager::new();
        let (app_halt_sender, app_halt_receiver) = halt::make_pair(HALT_TIMEOUT);
        let (managers, monitor) = Self::start_miner(
            &gpio_mgr,
            Self::detect_hashboards(&gpio_mgr).expect("failed detecting hashboards"),
            work_hub,
            backend_config,
            app_halt_receiver,
            app_halt_sender.clone(),
        )
        .await;

        // On miner exit, halt the whole program
        app_halt_sender
            .add_exit_hook(async {
                println!("Exiting.");
                std::process::exit(0);
            })
            .await;
        // Hook `Ctrl-C`, `SIGTERM` and other termination methods
        app_halt_sender.hook_termination_signals();

        Ok(hal::FrontendConfig {
            cgminer_custom_commands: cgminer::create_custom_commands(backend, managers, monitor),
        })
    }

    async fn init_work_solver(
        _backend_config: config::Backend,
        _work_solver: Arc<Self>,
    ) -> bosminer::Result<hal::FrontendConfig> {
        panic!("BUG: called `init_work_solver`");
    }
}

#[async_trait]
impl node::WorkSolver for Backend {
    async fn get_nominal_hashrate(&self) -> Option<ii_bitcoin::HashesUnit> {
        None
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bitmain Antminer S9")
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
    (secs * io::F_CLK_SPEED_HZ as f64) as u32
}
