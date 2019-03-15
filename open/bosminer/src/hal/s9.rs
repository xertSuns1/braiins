use nix::sys::mman::{MapFlags, ProtFlags};

use std::fs::OpenOptions;
use std::io;
use std::mem::size_of;
use std::os::unix::io::AsRawFd;
// TODO: remove thread specific components
use std::thread;

use std::time::Duration;
use std::time::SystemTime;

use slog::{error, info, trace};

use crate::misc::LOGGER;

use byteorder::{ByteOrder, LittleEndian};
use packed_struct::{PackedStruct, PackedStructSlice};

use embedded_hal::digital::InputPin;
use embedded_hal::digital::OutputPin;

use s9_io::hchainio0;

mod bm1387;
pub mod gpio;
pub mod power;

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

/// Hash Chain Controller provides abstraction of the FPGA interface for operating hashing boards.
/// It is the user-space driver for the IP Core
///
/// Main responsibilities:
/// - memory mapping of the FPGA control interface
/// - mining work submission and solution processing
///
/// TODO: implement drop trait (results in unmap)
/// TODO: rename to HashBoardCtrl and get rid of the hash_chain identifiers + array
pub struct HChainCtl<'a, VBackend> {
    hash_chain_ios: [&'a hchainio0::RegisterBlock; 2],
    /// Current work ID once it rolls over, we can start retiring old jobs
    work_id: u16,
    /// Number of chips that have been detected
    chip_count: usize,
    /// Eliminates the need to query the IP core about the current number of configured midstates
    midstate_count_bits: u8,
    /// Voltage controller on this hashboard
    /// TODO: consider making voltage ctrl a shared instance so that heartbeat and regular
    /// processing can use it. More: the backend should also become shared instance?
    voltage_ctrl: power::VoltageCtrl<VBackend>,
    #[allow(dead_code)]
    /// Plug pin that indicates the hashboard is present
    plug_pin: gpio::PinIn,
    /// Pin for resetting the hashboard
    rst_pin: gpio::PinOut,
    #[allow(dead_code)]
    /// When the heartbeat was last sent
    last_heartbeat_sent: Option<SystemTime>,
}

impl<'a, VBackend> HChainCtl<'a, VBackend>
where
    VBackend: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    /// Performs memory mapping of IP core's register block
    /// # TODO
    /// Research why custom flags - specifically O_SYNC and O_LARGEFILE fail
    fn mmap() -> Result<*const hchainio0::RegisterBlock, io::Error> {
        let mem_file = //File::open(path)?;
            OpenOptions::new().read(true).write(true)
                //.custom_flags(libc::O_RDWR | libc::O_SYNC | libc::O_LARGEFILE)
                .open("/dev/mem")?;

        let mmap = unsafe {
            nix::sys::mman::mmap(
                0 as *mut libc::c_void,
                4096,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                mem_file.as_raw_fd(),
                s9_io::HCHAINIO0::ptr() as libc::off_t,
            )
        };
        mmap.map(|addr| addr as *const hchainio0::RegisterBlock)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("mmap error! {:?}", e)))
    }

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
    ) -> Result<Self, io::Error> {
        // Hashboard creation is aborted if the pin is not present
        let plug_pin = gpio_mgr
            .get_pin_in(gpio::PinInName::Plug(hashboard_idx))
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "Hashboard {} failed to initialize plug pin: {}",
                        hashboard_idx, e
                    ),
                )
            })?;
        // also detect that the board is present
        if plug_pin.is_low() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Hashboard {} not present", hashboard_idx),
            ));
        }

        // Instantiate the reset pin
        let rst_pin = gpio_mgr
            .get_pin_out(gpio::PinOutName::Rst(hashboard_idx))
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "Hashboard {}: failed to initialize reset pin: {}",
                        hashboard_idx, e
                    ),
                )
            })?;

        let hash_chain_io = Self::mmap()?;
        let hash_chain_io = unsafe { &*hash_chain_io };

        Result::Ok(Self {
            hash_chain_ios: [hash_chain_io, hash_chain_io],
            work_id: 0,
            chip_count: 0,
            midstate_count_bits: midstate_count._bits(),
            voltage_ctrl: power::VoltageCtrl::new(voltage_ctrl_backend, hashboard_idx),
            plug_pin,
            rst_pin,
            last_heartbeat_sent: None,
        })
    }

    /// Helper method that initializes the FPGA IP core
    fn ip_core_init(&self) -> Result<(), io::Error> {
        // Disable ip core
        self.disable_ip_core();
        self.enable_ip_core();

        self.set_ip_core_baud_rate(INIT_CHIP_BAUD_RATE)?;
        // TODO consolidate hardcoded constant - calculate time constant based on PLL settings etc.
        self.set_ip_core_work_time(350000);
        self.set_ip_core_midstate_count();

        Ok(())
    }

    /// Puts the board into reset mode and disables the associated IP core
    fn enter_reset(&mut self) {
        self.disable_ip_core();
        // perform reset of the hashboard
        self.rst_pin.set_low();
    }

    /// Leaves reset mode
    fn exit_reset(&mut self) {
        self.rst_pin.set_high();
        self.enable_ip_core();
    }

    /// Initializes the complete hashboard including enumerating all chips
    pub fn init(&mut self) -> Result<(), io::Error> {
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
            let err_msg = format!(
                "Unexpected voltage controller firmware version: {}, expected: {}",
                version,
                power::EXPECTED_VOLTAGE_CTRL_VERSION
            );
            error!(LOGGER, "{}", err_msg);
            return Err(io::Error::new(io::ErrorKind::Other, err_msg));
        }
        // Voltage controller successfully initialized at this point, we should start sending
        // heart beats to it. Otherwise, it would shut down in about 10 seconds.
        info!(LOGGER, "Starting voltage controller heart beat task");
        let _ = self.voltage_ctrl.start_heart_beat_task();

        self.voltage_ctrl.set_voltage(6)?;
        self.voltage_ctrl.enable_voltage()?;
        info!(LOGGER, "Resetting hash board");
        self.enter_reset();
        // disable voltage
        self.voltage_ctrl.disable_voltage()?;
        thread::sleep(Duration::from_millis(INIT_DELAY_MS));
        self.voltage_ctrl.enable_voltage()?;
        thread::sleep(Duration::from_millis(2 * INIT_DELAY_MS));

        // TODO consider including a delay
        self.exit_reset();
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

        Ok(())
    }

    #[inline]
    pub fn is_work_tx_fifo_full(&self) -> bool {
        let hash_chain_io = self.hash_chain_ios[0];
        hash_chain_io.stat_reg.read().work_tx_full().bit()
    }

    /// Detects the number of chips on the hashing chain and assigns an address to each chip
    fn enumerate_chips(&mut self) -> Result<(), io::Error> {
        // Enumerate all chips (broadcast read address register request)
        let get_addr_cmd = bm1387::GetStatusCmd::new(0, true, bm1387::GET_ADDRESS_REG).pack();
        self.send_ctl_cmd(&get_addr_cmd, true);
        self.chip_count = 0;
        while let Some(addr_reg) = self.recv_ctl_cmd_resp::<bm1387::GetAddressReg>()? {
            if addr_reg.chip_rev != bm1387::ChipRev::Bm1387 {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "Unexpected revision of chip {} (expected: {:?} received: {:?})",
                        self.chip_count,
                        addr_reg.chip_rev,
                        bm1387::ChipRev::Bm1387
                    ),
                ));
            }
            self.chip_count += 1;
        }

        if self.chip_count >= MAX_CHIPS_ON_CHAIN {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Detected {} chips, expected less than 256 chips on 1 chain. Possibly a hardware issue?",
                    self.chip_count
                ),
            ));
        }
        if self.chip_count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "No chips detected on the current chain.",
            ));
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
    fn for_all_chips<F, R>(&self, f: F) -> Result<R, io::Error>
    where
        F: Fn(u8) -> Result<R, io::Error>,
    {
        let mut result: Result<R, io::Error> =
            Err(io::Error::new(io::ErrorKind::Other, "no chips to iterate"));
        for addr in (0..self.chip_count * 4).step_by(4) {
            // the enumeration takes care that address always fits into 8 bits.
            // Therefore, we can truncate the bits here.
            result = Ok(f(addr as u8)?);
        }
        // Result of last iteration
        result
    }

    /// Loads PLL register with a starting value
    fn set_pll(&self) -> Result<(), io::Error> {
        self.for_all_chips(|addr| {
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
    ) -> Result<usize, io::Error> {
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

    fn enable_ip_core(&self) {
        self.hash_chain_ios[0]
            .ctrl_reg
            .modify(|_, w| w.enable().bit(true));
    }

    fn disable_ip_core(&self) {
        self.hash_chain_ios[0]
            .ctrl_reg
            .modify(|_, w| w.enable().bit(false));
    }

    fn set_ip_core_work_time(&self, work_time: u32) {
        self.hash_chain_ios[0]
            .work_time
            .write(|w| unsafe { w.bits(work_time) });
    }

    /// This method only changes the communication speed of the FPGA IP core with the chips.
    ///
    /// Note: change baud rate of the FPGA is only desirable as a step after all chips in the
    /// chain have been reconfigured for a different speed, too.
    fn set_ip_core_baud_rate(&self, baud: usize) -> Result<(), io::Error> {
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

        self.hash_chain_ios[0]
            .baud_reg
            .write(|w| unsafe { w.bits(baud_clock_div as u32) });
        Ok(())
    }

    fn set_ip_core_midstate_count(&self) {
        self.hash_chain_ios[0]
            .ctrl_reg
            .modify(|_, w| unsafe { w.midstate_cnt().bits(self.midstate_count_bits) });
    }

    fn u256_as_u32_slice(src: &uint::U256) -> &[u32] {
        unsafe {
            core::slice::from_raw_parts(
                src.0.as_ptr() as *const u32,
                size_of::<uint::U256>() / size_of::<u32>(),
            )
        }
    }

    #[inline]
    /// Work ID's are generated with a step that corresponds to the number of configured midstates
    fn next_work_id(&mut self) -> u32 {
        let retval = self.work_id as u32;
        // compiler has to know that work ID rolls over regularly
        self.work_id = self.work_id.wrapping_add(1 << self.midstate_count_bits);
        retval
    }

    #[inline]
    /// TODO: implement error handling/make interface ready for ASYNC execution
    /// Writes single word into a TX fifo
    fn write_to_work_tx_fifo(&self, item: u32) {
        let hash_chain_io = self.hash_chain_ios[0];
        while self.is_work_tx_fifo_full() {}
        hash_chain_io
            .work_tx_fifo
            .write(|w| unsafe { w.bits(item) });
    }

    #[inline]
    fn read_from_work_rx_fifo(&self) -> Result<u32, io::Error> {
        let hash_chain_io = self.hash_chain_ios[0];
        // TODO temporary workaround until we have asynchronous handling - wait 5 ms if the FIFO
        // is empty
        if hash_chain_io.stat_reg.read().work_rx_empty().bit() {
            thread::sleep(Duration::from_millis(5));
        }
        if hash_chain_io.stat_reg.read().work_rx_empty().bit() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Work RX fifo empty",
            ));
        }
        Ok(hash_chain_io.work_rx_fifo.read().bits())
    }

    #[inline]
    /// TODO get rid of busy waiting, prepare for non-blocking API
    fn write_to_cmd_tx_fifo(&self, item: u32) {
        let hash_chain_io = self.hash_chain_ios[0];
        while hash_chain_io.stat_reg.read().cmd_tx_full().bit() {}
        hash_chain_io.cmd_tx_fifo.write(|w| unsafe { w.bits(item) });
    }

    #[inline]
    fn read_from_cmd_rx_fifo(&self) -> Result<u32, io::Error> {
        let hash_chain_io = self.hash_chain_ios[0];
        // TODO temporary workaround until we have asynchronous handling - wait 5 ms if the FIFO
        // is empty
        trace!(LOGGER, "Checking CMD RX FIFO empty bit");
        if hash_chain_io.stat_reg.read().cmd_rx_empty().bit() {
            thread::sleep(Duration::from_millis(5));
        }
        if hash_chain_io.stat_reg.read().cmd_rx_empty().bit() {
            trace!(LOGGER, "Reading CMD RX FIFO timed out!");
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Command RX fifo empty, read has timedout",
            ));
        }
        Ok(hash_chain_io.cmd_rx_fifo.read().bits())
    }

    #[inline]
    /// Helper function that extracts work ID from the solution ID
    pub fn get_work_id_from_solution_id(&self, solution_id: u32) -> u32 {
        ((solution_id >> WORK_ID_OFFSET) >> self.midstate_count_bits)
    }

    #[inline]
    /// Extracts midstate index from the solution ID
    fn get_midstate_idx_from_solution_id(&self, solution_id: u32) -> usize {
        ((solution_id >> WORK_ID_OFFSET) & ((1u32 << self.midstate_count_bits) - 1)) as usize
    }

    #[inline]
    /// Extracts solution index from the solution ID
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
            self.write_to_cmd_tx_fifo(LittleEndian::read_u32(chunk));
        }
        // TODO busy waiting has to be replaced once asynchronous processing is in place
        if wait {
            while !self.hash_chain_ios[0].stat_reg.read().cmd_tx_empty().bit() {}
        }
    }

    /// Command responses are always 7 bytes long including checksum. Therefore, the reception
    /// has to be done in 2 steps with the following error handling:
    ///
    /// - A timeout when reading the first word is converted into an empty response.
    ///   The method propagates any error other than timeout
    /// - An error that occurs during reading the second word from the FIFO is propagated.
    fn recv_ctl_cmd_resp<T: PackedStructSlice>(&self) -> Result<Option<T>, io::Error> {
        let mut cmd_resp = [0u8; 8];

        // TODO: to be refactored once we have asynchronous handling in place
        // fetch command response from IP core's fifo
        match self.read_from_cmd_rx_fifo() {
            Err(e) => {
                if e.kind() == io::ErrorKind::TimedOut {
                    return Ok(None);
                } else {
                    return Err(e);
                }
            }
            Ok(resp_word) => LittleEndian::write_u32(&mut cmd_resp[..4], resp_word),
        }
        // All errors from reading the second word are propagated
        let resp_word = self.read_from_cmd_rx_fifo()?;
        LittleEndian::write_u32(&mut cmd_resp[4..], resp_word);

        // build the response instance - drop the extra byte due to FIFO being 32-bit word based
        // and drop the checksum
        // TODO: optionally verify the checksum (use debug_assert?)
        T::unpack_from_slice(&cmd_resp[..6])
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "Control command unpacking error! {:?} {:#04x?}",
                        e, cmd_resp
                    ),
                )
            })
            .map(|resp| Some(resp))
    }
}

impl<'a, VBackend> super::HardwareCtl for HChainCtl<'a, VBackend>
where
    VBackend: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    fn send_work(&mut self, work: &super::MiningWork) -> Result<u32, io::Error> {
        let work_id = self.next_work_id();

        self.write_to_work_tx_fifo(work_id);
        self.write_to_work_tx_fifo(work.nbits);
        self.write_to_work_tx_fifo(work.ntime);
        self.write_to_work_tx_fifo(work.merkel_root_lsw);

        for midstate in work.midstates.iter() {
            let midstate = HChainCtl::<VBackend>::u256_as_u32_slice(&midstate);
            // Chip expects the midstate in reverse word order
            for midstate_word in midstate.iter().rev() {
                self.write_to_work_tx_fifo(*midstate_word);
            }
        }
        Ok(work_id)
    }

    fn recv_solution(&mut self) -> Result<Option<super::MiningWorkSolution>, io::Error> {
        let nonce; // = self.read_from_work_rx_fifo()?;
                   // TODO: to be refactored once we have asynchronous handling in place
                   // fetch command response from IP core's fifo
        match self.read_from_work_rx_fifo() {
            Err(e) => {
                if e.kind() == io::ErrorKind::TimedOut {
                    return Ok(None);
                } else {
                    return Err(e);
                }
            }
            Ok(resp_word) => nonce = resp_word,
        }

        let word2 = self.read_from_work_rx_fifo()?;

        let solution = super::MiningWorkSolution {
            nonce,
            // this hardware doesn't do any nTime rolling, keep it @ None
            ntime: None,
            midstate_idx: self.get_midstate_idx_from_solution_id(word2),
            // leave the result ID as-is so that we can extract solution index etc later.
            solution_id: word2 & 0xffffffu32,
        };

        Ok(Some(solution))
    }

    fn get_work_id_from_solution(&self, solution: &super::MiningWorkSolution) -> u32 {
        self.get_work_id_from_solution_id(solution.solution_id)
    }

    fn get_chip_count(&self) -> usize {
        self.chip_count
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
) -> Result<(usize, usize), io::Error> {
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
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "Invalid baudrate - requested: {} baud, resulting {} baud",
                baud_rate, actual_baud_rate
            ),
        ));
    }
    Ok((baud_div, actual_baud_rate))
}

#[cfg(test)]
mod test {
    use super::*;
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
        let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::<I2cdev>::new(0);
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
        let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::<I2cdev>::new(0);
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
            h_chain_ctl.hash_chain_ios[0].work_time.read().bits(),
            350000,
            "Unexpected work time value"
        );
        assert_eq!(
            h_chain_ctl.hash_chain_ios[0].baud_reg.read().bits(),
            0x1a,
            "Unexpected baud rate register value for {} baud",
            INIT_CHIP_BAUD_RATE
        );
        assert_eq!(
            h_chain_ctl.hash_chain_ios[0].stat_reg.read().bits(),
            0x855,
            "Unexpected status register value"
        );
        assert_eq!(
            h_chain_ctl.hash_chain_ios[0].ctrl_reg.read().midstate_cnt(),
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
            let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::<I2cdev>::new(0);
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

}
