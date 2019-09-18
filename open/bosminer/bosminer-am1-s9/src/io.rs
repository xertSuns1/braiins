// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

//! This module provides API to access memory-mapped FPGA registers and associated interrupts.
//!
//! It is split into two layers:
//!   * `Io` layer, which provides interface to FPGA registers and implements
//!     API to wait for events (via interrupts)
//!   * `Control` layer knows about chip configuration (number of midstates)
//!     and implements few higher-level functions to read/write work

mod ext_work_id;
mod uio;

use crate::error::{self, ErrorKind};
use crate::MidstateCount;
use ext_work_id::ExtWorkId;

use bosminer::work;
use byteorder::{ByteOrder, LittleEndian};
use std::time::Duration;

use ii_async_compat::{sleep, timeout_future, TimeoutResult};
use ii_fpga_io_am1_s9::hchainio0;

use ii_logging::macros::*;

/// What bitstream version do we expect
const EXPECTED_BITSTREAM_BUILD_ID: u32 = 0x5D5E7158;

/// XXX: this function will be gone once DTS is fixed.
fn map_mem_regs<T>(
    hashboard_idx: usize,
    name: &'static str,
) -> error::Result<(uio_async::UioTypedMapping<T>, uio_async::UioDevice)> {
    let regs: uio_async::UioTypedMapping<T> = uio::Device::open(hashboard_idx, "mem")?.map()?;
    let uio = uio::Device::open(hashboard_idx, name)?.uio;
    Ok((regs, uio))
}

pub struct ConfigHw {
    regs: uio_async::UioTypedMapping<hchainio0::RegisterBlock>,
}

impl ConfigHw {
    /// Return build id (unix timestamp) of s9-hw FPGA bitstream
    #[inline]
    pub fn get_build_id(&mut self) -> u32 {
        self.regs.build_id.read().bits()
    }

    pub fn init(&self) -> error::Result<()> {
        Ok(())
    }

    pub fn enable_ip_core(&self) {
        self.regs.ctrl_reg.modify(|_, w| w.enable().bit(true));
    }

    pub fn disable_ip_core(&self) {
        self.regs.ctrl_reg.modify(|_, w| w.enable().bit(false));
    }

    pub fn set_ip_core_work_time(&self, work_time: u32) {
        self.regs.work_time.write(|w| unsafe { w.bits(work_time) });
    }

    pub fn set_baud_clock_div(&self, baud_clock_div: u32) {
        self.regs
            .baud_reg
            .write(|w| unsafe { w.bits(baud_clock_div) });
    }

    /// XXX: not sure if we should leak the `ctrl_reg` type here
    /// (of course we shouldn't but who is the responsible for the translation?)
    pub fn set_ip_core_midstate_count(&self, value: hchainio0::ctrl_reg::MIDSTATE_CNT_A) {
        self.regs
            .ctrl_reg
            .modify(|_, w| w.midstate_cnt().variant(value));
    }

    pub fn new(hashboard_idx: usize) -> error::Result<Self> {
        let regs = uio::Device::open(hashboard_idx, "mem")?.map()?;
        Ok(Self { regs })
    }
}

struct WorkRxFifo {
    regs: uio_async::UioTypedMapping<hchainio0::RegisterBlock>,
    uio: uio_async::UioDevice,
}

impl WorkRxFifo {
    #[inline]
    pub fn fifo_empty(&self) -> bool {
        self.regs.stat_reg.read().work_rx_empty().bit()
    }

    /// Try to read from work rx fifo.
    /// Performs blocking read with timeout. Uses IRQ.
    #[allow(dead_code)]
    #[inline]
    pub fn read(&mut self, timeout: Option<Duration>) -> error::Result<Option<u32>> {
        let cond = || !self.fifo_empty();
        let got_irq = self.uio.irq_wait_cond(cond, timeout)?;
        Ok(got_irq.and_then(|_| Some(self.regs.work_rx_fifo.read().bits())))
    }

    /// Try to read from work rx fifo.
    /// Async variant. Uses IRQ.
    pub async fn async_read(&mut self) -> error::Result<u32> {
        let cond = || !self.fifo_empty();
        await!(self.uio.async_irq_wait_cond(cond))?;
        Ok(self.regs.work_rx_fifo.read().bits())
    }

    pub fn init(&mut self) -> error::Result<()> {
        // reset input FIFO
        self.regs
            .ctrl_reg
            .modify(|_, w| w.rst_work_rx_fifo().set_bit());
        // enable IRQ_WORK_RX interrupt
        self.regs
            .ctrl_reg
            .modify(|_, w| w.irq_en_work_rx().set_bit());
        Ok(())
    }

    pub fn new(hashboard_idx: usize) -> error::Result<Self> {
        let (regs, uio) = map_mem_regs(hashboard_idx, "work-rx")?;
        Ok(Self { regs, uio })
    }
}

struct WorkTxFifo {
    regs: uio_async::UioTypedMapping<hchainio0::RegisterBlock>,
    uio: uio_async::UioDevice,
}

impl WorkTxFifo {
    /// How big is FIFO queue? (in u32 words)
    const FIFO_WORK_TX_SIZE: u32 = 2048;

    /// How big is the absolute biggest "work"? (again, in u32 words)
    const FIFO_WORK_MAX_SIZE: u32 = 200;

    /// Threshold for number of entries in FIFO queue under which we recon we could
    /// fit one more work.
    const FIFO_WORK_TX_THRESHOLD: u32 = Self::FIFO_WORK_TX_SIZE - Self::FIFO_WORK_MAX_SIZE;

    #[inline]
    pub fn is_fifo_full(&self) -> bool {
        self.regs.stat_reg.read().work_tx_full().bit()
    }

    #[inline]
    pub fn has_space_for_one_job(&self) -> bool {
        self.regs.stat_reg.read().irq_pend_work_tx().bit()
    }

    /// Return the value of last work ID send to ASICs
    #[inline]
    #[allow(dead_code)]
    pub fn get_last_work_id(&mut self) -> u32 {
        self.regs.last_work_id.read().bits()
    }

    /// Try to write work item to work TX FIFO.
    /// Performs blocking write without timeout. Uses IRQ.
    /// The idea is that you don't call this function until you are sure you
    /// can fit in all the entries you want - for example
    /// `hash_work_tx_space_for_one_job`.
    #[inline]
    pub fn write(&mut self, item: u32) -> error::Result<()> {
        let cond = || !self.is_fifo_full();
        self.uio.irq_wait_cond(cond, None)?;
        self.regs.work_tx_fifo.write(|w| unsafe { w.bits(item) });
        Ok(())
    }

    /// Wait for output FIFO to make room for one work
    pub async fn async_wait_for_room(&self) -> error::Result<()> {
        let cond = || self.has_space_for_one_job();
        await!(self.uio.async_irq_wait_cond(cond))?;
        Ok(())
    }

    pub fn init(&mut self) -> error::Result<()> {
        // Set threshold for work TX so that there's space for
        // at least one job.
        self.regs
            .irq_fifo_thr
            .write(|w| unsafe { w.bits(Self::FIFO_WORK_TX_THRESHOLD) });
        // reset output FIFO
        self.regs
            .ctrl_reg
            .modify(|_, w| w.rst_work_tx_fifo().set_bit());
        // enable IRQ_WORK_TX interrupt
        self.regs
            .ctrl_reg
            .modify(|_, w| w.irq_en_work_tx().set_bit());
        Ok(())
    }

    pub fn new(hashboard_idx: usize) -> error::Result<Self> {
        let (regs, uio) = map_mem_regs(hashboard_idx, "work-tx")?;
        Ok(Self { regs, uio })
    }
}

/// This object drives both FIFOs, because we handle command responses
/// in a task synchronously.
///
/// TODO: Split this FIFO into two FIFOs.
pub struct CommandRxTxFifos {
    regs: uio_async::UioTypedMapping<hchainio0::RegisterBlock>,
    uio: uio_async::UioDevice,
}

impl CommandRxTxFifos {
    #[inline]
    pub fn is_rx_empty(&self) -> bool {
        self.regs.stat_reg.read().cmd_rx_empty().bit()
    }

    #[inline]
    pub fn is_tx_empty(&self) -> bool {
        self.regs.stat_reg.read().cmd_tx_empty().bit()
    }

    #[inline]
    pub fn is_tx_full(&self) -> bool {
        self.regs.stat_reg.read().cmd_tx_full().bit()
    }

    /// Wait for command FIFO to become empty
    /// Uses timed polling
    pub async fn wait_tx_empty(&self) {
        while !self.is_tx_empty() {
            await!(sleep(Duration::from_millis(1)));
        }
    }

    /// Write command to cmd tx fifo.
    /// Uses timed polling
    #[inline]
    pub async fn write(&self, item: u32) {
        // wait for space in queue
        while self.is_tx_full() {
            await!(sleep(Duration::from_millis(1)));
        }
        // write command word
        self.regs.cmd_tx_fifo.write(|w| unsafe { w.bits(item) });
    }

    /// Try to read command from cmd rx fifo.
    /// Performs blocking read with timeout. Uses IRQ.
    #[inline]
    pub fn read(&mut self, timeout: Option<Duration>) -> error::Result<Option<u32>> {
        let cond = || !self.is_rx_empty();
        let got_irq = self.uio.irq_wait_cond(cond, timeout)?;
        Ok(got_irq.and_then(|_| Some(self.regs.cmd_rx_fifo.read().bits())))
    }

    /// Read command from cmd rx fifo
    /// Async variant. Uses IRQ.
    pub async fn async_read(&mut self) -> error::Result<u32> {
        let cond = || !self.is_rx_empty();
        await!(self.uio.async_irq_wait_cond(cond))?;
        Ok(self.regs.cmd_rx_fifo.read().bits())
    }

    /// Read command from cmd rx fifo with timeout
    /// Async variant. Uses IRQ.
    /// Returns:
    ///     * `Ok(None)` on timeout
    ///     * `Ok(Some(_))` if something was received
    ///     * `Err(_)` if error occured
    pub async fn async_read_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> error::Result<Option<u32>> {
        match await!(timeout_future(self.async_read(), timeout,)) {
            TimeoutResult::Error => panic!("timeout error"),
            TimeoutResult::TimedOut => return Ok(None),
            TimeoutResult::Returned(word) => return Ok(Some(word?)),
        }
    }

    pub fn init(&mut self) -> error::Result<()> {
        // reset input FIFO
        self.regs
            .ctrl_reg
            .modify(|_, w| w.rst_cmd_rx_fifo().set_bit().rst_cmd_tx_fifo().set_bit());
        // enable IRQ_CMD_RX interrupt
        self.regs
            .ctrl_reg
            .modify(|_, w| w.irq_en_cmd_rx().set_bit());
        Ok(())
    }

    pub fn new(hashboard_idx: usize) -> error::Result<Self> {
        let (regs, uio) = map_mem_regs(hashboard_idx, "cmd-rx")?;
        Ok(Self { regs, uio })
    }
}

/// This structure represents mining solution response as read from
/// `WORK_RX_FIFO` in FPGA.
#[derive(Debug, Clone)]
struct WorkRxResponse {
    pub nonce: u32,
    pub work_id: usize,
    pub midstate_idx: usize,
    pub solution_idx: usize,
}

impl WorkRxResponse {
    /// Parse from FPGA response
    /// The format is dependent on current `MidstateCount` settings
    pub fn from_hw(midstate_count: MidstateCount, word1: u32, word2: u32) -> Self {
        // NOTE: there's a CRC field in word2 that we ignore, because it's checked by FPGA core
        let solution_idx = word2 & 0xff;
        let ext_work_id = (word2 >> 8) & 0xffff;
        let ext_work_id = ExtWorkId::from_hw(midstate_count, ext_work_id);
        Self {
            nonce: word1,
            solution_idx: solution_idx as usize,
            work_id: ext_work_id.work_id,
            midstate_idx: ext_work_id.midstate_idx,
        }
    }
}

pub struct WorkRx {
    hw: WorkRxFifo,
    midstate_count: MidstateCount,
}

impl WorkRx {
    pub async fn recv_solution(mut self) -> Result<(Self, work::Solution), failure::Error> {
        let word1 = await!(self.hw.async_read())?;
        let word2 = await!(self.hw.async_read())?;
        let resp = WorkRxResponse::from_hw(self.midstate_count, word1, word2);

        let solution = work::Solution {
            nonce: resp.nonce,
            ntime: None,
            midstate_idx: resp.midstate_idx,
            solution_idx: resp.solution_idx,
            hardware_id: resp.work_id as u32,
        };

        Ok((self, solution))
    }

    fn init(&mut self) -> error::Result<()> {
        self.hw.init()
    }

    fn new(hashboard_idx: usize, midstate_count: MidstateCount) -> error::Result<Self> {
        Ok(Self {
            hw: WorkRxFifo::new(hashboard_idx)?,
            midstate_count,
        })
    }
}

pub struct WorkTx {
    hw: WorkTxFifo,
    midstate_count: MidstateCount,
}

impl WorkTx {
    pub async fn wait_for_room(&self) -> error::Result<()> {
        await!(self.hw.async_wait_for_room())
    }

    pub fn assert_midstate_count(&self, expected_midstate_count: usize) {
        assert_eq!(
            expected_midstate_count,
            self.midstate_count.to_count(),
            "Outgoing work has {} midstates, but miner is configured for {} midstates!",
            expected_midstate_count,
            self.midstate_count.to_count(),
        );
    }

    pub fn send_work(
        &mut self,
        work: &work::Assignment,
        work_id: usize,
    ) -> Result<(), failure::Error> {
        self.assert_midstate_count(work.midstates.len());
        let ext_work_id = ExtWorkId::new(work_id, 0);

        self.hw
            .write(ext_work_id.to_hw(self.midstate_count).to_le())?;
        self.hw.write(work.bits().to_le())?;
        self.hw.write(work.ntime.to_le())?;
        self.hw.write(work.merkle_root_tail().to_le())?;

        for mid in work.midstates.iter() {
            for midstate_word in mid.state.words::<u32>().rev() {
                self.hw.write(midstate_word.to_be())?;
            }
        }
        Ok(())
    }

    /// Return upper bound for `work_id`
    /// Determines how big the work registry has to be
    pub fn work_id_count(&self) -> usize {
        ExtWorkId::get_work_id_count(self.midstate_count)
    }

    fn init(&mut self) -> error::Result<()> {
        self.hw.init()
    }

    fn new(hashboard_idx: usize, midstate_count: MidstateCount) -> error::Result<Self> {
        Ok(Self {
            hw: WorkTxFifo::new(hashboard_idx)?,
            midstate_count,
        })
    }
}

pub struct CommandRxTx {
    hw: CommandRxTxFifos,
}

impl CommandRxTx {
    /// Timeout for waiting for command
    const COMMAND_READ_TIMEOUT: Duration = Duration::from_millis(100);

    /// Serializes command into 32-bit words and submits it to the command TX FIFO
    ///
    /// * `wait` - when true, wait until all commands are sent
    pub async fn send_command(&self, cmd: Vec<u8>, wait: bool) {
        // invariant required by the IP core
        assert_eq!(
            cmd.len() & 0x3,
            0,
            "Control command length not aligned to 4 byte boundary!"
        );
        trace!("Sending Control Command {:x?}", cmd);
        for chunk in cmd.chunks(4) {
            await!(self.hw.write(LittleEndian::read_u32(chunk)));
        }
        if wait {
            await!(self.hw.wait_tx_empty());
        }
    }

    /// Receive command response.
    /// Command responses are always 7 bytes long including checksum. Therefore, the reception
    /// has to be done in 2 steps with the following error handling:
    ///
    /// - A timeout when reading the first word is converted into an empty response.
    ///   The method propagates any error other than timeout
    /// - An error that occurs during reading the second word from the FIFO is propagated.
    pub async fn recv_response(&mut self) -> error::Result<Option<Vec<u8>>> {
        // assembled response
        let mut cmd_resp = [0u8; 8];

        // fetch first word of command response from IP core's fifo
        match await!(self.hw.async_read_with_timeout(Self::COMMAND_READ_TIMEOUT))? {
            None => return Ok(None),
            Some(word) => LittleEndian::write_u32(&mut cmd_resp[..4], word),
        }

        // fetch second word: getting timeout here is a hardware error
        match await!(self.hw.async_read_with_timeout(Self::COMMAND_READ_TIMEOUT))? {
            None => Err(ErrorKind::Fifo(
                error::Fifo::TimedOut,
                "cmd RX fifo framing error".to_string(),
            ))?,
            Some(word2) => LittleEndian::write_u32(&mut cmd_resp[4..], word2),
        }

        // build the response vector - drop the extra byte due to FIFO being 32-bit word based
        // and drop the checksum
        // TODO: optionally verify the checksum (use debug_assert?)
        Ok(Some(cmd_resp[..6].to_vec()))
    }

    fn init(&mut self) -> error::Result<()> {
        self.hw.init()?;
        Ok(())
    }

    fn new(hashboard_idx: usize) -> error::Result<Self> {
        Ok(Self {
            hw: CommandRxTxFifos::new(hashboard_idx)?,
        })
    }
}

pub struct Config {
    hw: ConfigHw,
    midstate_count: MidstateCount,
}

impl Config {
    fn check_build_id(&mut self) -> error::Result<()> {
        let build_id = self.hw.get_build_id();
        if build_id != EXPECTED_BITSTREAM_BUILD_ID {
            Err(ErrorKind::UnexpectedVersion(
                "s9-hw bitstream".to_string(),
                format!("0x{:08x}", build_id),
                format!("0x{:08x}", EXPECTED_BITSTREAM_BUILD_ID),
            ))?
        }
        Ok(())
    }

    pub fn set_midstate_count(&self) {
        self.hw
            .set_ip_core_midstate_count(self.midstate_count.to_reg());
    }

    pub fn enable_ip_core(&self) {
        self.hw.enable_ip_core();
    }

    pub fn disable_ip_core(&self) {
        self.hw.disable_ip_core();
    }

    pub fn set_ip_core_work_time(&self, work_time: u32) {
        self.hw.set_ip_core_work_time(work_time);
    }

    pub fn set_baud_clock_div(&self, baud_clock_div: u32) {
        self.hw.set_baud_clock_div(baud_clock_div);
    }

    fn init(&mut self) -> error::Result<()> {
        self.hw.init()?;
        self.check_build_id()?;
        Ok(())
    }

    fn new(hashboard_idx: usize, midstate_count: MidstateCount) -> error::Result<Self> {
        Ok(Self {
            hw: ConfigHw::new(hashboard_idx)?,
            midstate_count,
        })
    }
}

/// Represents the whole IP core
pub struct Core {
    config_io: Config,
    command_io: CommandRxTx,
    work_rx_io: WorkRx,
    work_tx_io: WorkTx,
}

impl Core {
    /// Build a new IP core
    pub fn new(hashboard_idx: usize, midstate_count: MidstateCount) -> error::Result<Self> {
        Ok(Self {
            config_io: Config::new(hashboard_idx, midstate_count)?,
            command_io: CommandRxTx::new(hashboard_idx)?,
            work_rx_io: WorkRx::new(hashboard_idx, midstate_count)?,
            work_tx_io: WorkTx::new(hashboard_idx, midstate_count)?,
        })
    }

    /// Initialize the IP core and split it into components
    /// That way it's not possible to access un-initialized IO blocks
    pub fn init_and_split(mut self) -> error::Result<(Config, CommandRxTx, WorkRx, WorkTx)> {
        // Reset IP core
        self.config_io.init()?;
        self.config_io.disable_ip_core();
        self.config_io.enable_ip_core();

        // Initialize fifos
        self.command_io.init()?;
        self.work_rx_io.init()?;
        self.work_tx_io.init()?;

        Ok((
            self.config_io,
            self.command_io,
            self.work_rx_io,
            self.work_tx_io,
        ))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    /// Index of chain for testing (must exist and be defined in DTS)
    const TEST_CHAIN_INDEX: usize = 8;

    /// Test that we are able to construct HChainFifo instance
    #[test]
    fn test_fifo_initialization() {
        let core =
            Core::new(TEST_CHAIN_INDEX, MidstateCount::new(1)).expect("fifo construction failed");
        core.init_and_split().expect("fifo initialization failed");
    }
}

#[cfg(test)]
pub mod test_utils {
    use super::*;

    /// Represents configuration of Config block
    pub struct ConfigRegs {
        pub work_time: u32,
        pub baud_reg: u32,
        pub stat_reg: u32,
        pub midstate_cnt: u32,
    }

    impl ConfigRegs {
        pub fn new(io: &Config) -> Self {
            Self {
                work_time: io.hw.regs.work_time.read().bits(),
                baud_reg: io.hw.regs.baud_reg.read().bits(),
                stat_reg: io.hw.regs.stat_reg.read().bits(),
                midstate_cnt: 1u32 << io.hw.regs.ctrl_reg.read().midstate_cnt().bits(),
            }
        }
    }

    /// This test verifies correct parsing of mining work solution for all multi-midstate
    /// configurations.
    /// The solution_word represents the second word of data provided that follows the nonce as
    /// provided by the FPGA IP core
    #[test]
    fn test_work_rx_response() {
        let word1 = 0xdead0666;
        let word2 = 0x98123502;
        struct ExpectedSolutionData {
            work_id: usize,
            midstate_idx: usize,
            solution_idx: usize,
            midstate_count: MidstateCount,
        };
        let expected_solution_data = [
            ExpectedSolutionData {
                work_id: 0x1235,
                midstate_idx: 0,
                solution_idx: 2,
                midstate_count: MidstateCount::new(1),
            },
            ExpectedSolutionData {
                work_id: 0x091a,
                midstate_idx: 1,
                solution_idx: 2,
                midstate_count: MidstateCount::new(2),
            },
            ExpectedSolutionData {
                work_id: 0x048d,
                midstate_idx: 1,
                solution_idx: 2,
                midstate_count: MidstateCount::new(4),
            },
        ];
        for (i, expected_solution_data) in expected_solution_data.iter().enumerate() {
            // The midstate configuration (ctrl_reg::MIDSTATE_CNT_W) doesn't implement a debug
            // trait. Therefore, we extract only those parts that can be easily displayed when a
            // test failed.
            let expected_data = (
                expected_solution_data.work_id,
                expected_solution_data.midstate_idx,
                expected_solution_data.solution_idx,
            );
            let resp = WorkRxResponse::from_hw(expected_solution_data.midstate_count, word1, word2);

            assert_eq!(resp.nonce, word1);
            assert_eq!(
                resp.work_id, expected_solution_data.work_id,
                "Invalid work ID, iteration: {}, test data: {:#06x?}",
                i, expected_data
            );
            assert_eq!(
                resp.midstate_idx, expected_solution_data.midstate_idx,
                "Invalid midstate index, iteration: {}, test data: {:#06x?}",
                i, expected_data
            );
            assert_eq!(
                resp.solution_idx, expected_solution_data.solution_idx,
                "Invalid solution index, iteration: {}, test data: {:#06x?}",
                i, expected_data
            );
        }
    }

}
