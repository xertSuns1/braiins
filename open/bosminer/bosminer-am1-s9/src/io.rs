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

mod tag;
mod uio;

use crate::error::{self, ErrorKind};
use crate::MidstateCount;

use bosminer::work;
use std::time::Duration;

use ii_fpga_io_am1_s9::hchainio0;

/// What bitstream version do we expect
const EXPECTED_BITSTREAM_BUILD_ID: u32 = 0x5D5E7158;
/// How long to wait for RX interrupt
const FIFO_READ_TIMEOUT: Duration = Duration::from_millis(5);

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

struct WorkRxHw {
    regs: uio_async::UioTypedMapping<hchainio0::RegisterBlock>,
    uio: uio_async::UioDevice,
}

struct WorkTxHw {
    regs: uio_async::UioTypedMapping<hchainio0::RegisterBlock>,
    uio: uio_async::UioDevice,
}

pub struct CommandHw {
    regs: uio_async::UioTypedMapping<hchainio0::RegisterBlock>,
    uio: uio_async::UioDevice,
}

pub struct WorkRxIo {
    hw: WorkRxHw,
    tag_manager: tag::TagManager,
}

pub struct WorkTxIo {
    hw: WorkTxHw,
    tag_manager: tag::TagManager,
}

pub struct ConfigIo {
    hw: ConfigHw,
    midstate_count: MidstateCount,
}

/// TODO: make `hw` private
pub struct CommandIo {
    pub hw: CommandHw,
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

impl WorkRxHw {
    #[inline]
    pub fn fifo_empty(&self) -> bool {
        self.regs.stat_reg.read().work_rx_empty().bit()
    }

    /// Try to read from work rx fifo.
    /// Performs blocking read with timeout. Uses IRQ.
    #[inline]
    pub fn read(&mut self) -> error::Result<Option<u32>> {
        let cond = || !self.fifo_empty();
        let got_irq = self.uio.irq_wait_cond(cond, Some(FIFO_READ_TIMEOUT))?;
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

impl WorkTxHw {
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

impl CommandHw {
    #[inline]
    pub fn is_rx_empty(&self) -> bool {
        self.regs.stat_reg.read().cmd_rx_empty().bit()
    }

    #[inline]
    pub fn is_tx_empty(&self) -> bool {
        self.regs.stat_reg.read().cmd_tx_empty().bit()
    }

    /// Wait fro command FIFO to become empty.
    /// Uses polling.
    pub async fn wait_tx_empty(&self) {
        // async "busy-wait", since there's no command TX interrupt
        while !self.is_tx_empty() {
            await!(ii_async_compat::sleep(Duration::from_millis(2)));
        }
    }

    /// Try to write command to cmd tx fifo.
    /// Performs blocking write without timeout. Uses polling.
    /// TODO get rid of busy waiting, prepare for non-blocking API
    #[inline]
    pub fn write(&self, item: u32) {
        while self.regs.stat_reg.read().cmd_tx_full().bit() {}
        self.regs.cmd_tx_fifo.write(|w| unsafe { w.bits(item) });
    }

    /// Try to read command from cmd rx fifo.
    /// Performs blocking read with timeout. Uses IRQ.
    #[inline]
    pub fn read(&mut self) -> error::Result<Option<u32>> {
        let cond = || !self.is_rx_empty();
        let got_irq = self.uio.irq_wait_cond(cond, Some(FIFO_READ_TIMEOUT))?;
        Ok(got_irq.and_then(|_| Some(self.regs.cmd_rx_fifo.read().bits())))
    }

    pub async fn async_read(&mut self) -> error::Result<u32> {
        let cond = || !self.is_rx_empty();
        await!(self.uio.async_irq_wait_cond(cond))?;
        Ok(self.regs.cmd_rx_fifo.read().bits())
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

impl WorkRxIo {
    pub async fn recv_solution(mut self) -> Result<(Self, work::Solution), failure::Error> {
        let nonce = await!(self.hw.async_read())?;
        let word2 = await!(self.hw.async_read())?;
        let output_tag = self.tag_manager.parse_output_tag(word2);

        let solution = work::Solution {
            nonce,
            ntime: None,
            midstate_idx: output_tag.midstate_idx,
            solution_idx: output_tag.solution_idx,
            hardware_id: output_tag.work_id as u32,
        };

        Ok((self, solution))
    }

    pub fn init(&mut self) -> error::Result<()> {
        self.hw.init()
    }

    pub fn new(hashboard_idx: usize, midstate_count: MidstateCount) -> error::Result<Self> {
        Ok(Self {
            hw: WorkRxHw::new(hashboard_idx)?,
            tag_manager: tag::TagManager::new(midstate_count),
        })
    }
}

impl WorkTxIo {
    pub async fn wait_for_room(&self) -> error::Result<()> {
        await!(self.hw.async_wait_for_room())
    }

    pub fn send_work(
        &mut self,
        work: &work::Assignment,
        work_id: usize,
    ) -> Result<(), failure::Error> {
        let input_tag = self
            .tag_manager
            .make_input_tag(work_id, work.midstates.len());

        self.hw.write(input_tag.to_le())?;
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
    pub fn work_id_range(&self) -> usize {
        self.tag_manager.work_id_range()
    }

    pub fn init(&mut self) -> error::Result<()> {
        self.hw.init()
    }

    pub fn new(hashboard_idx: usize, midstate_count: MidstateCount) -> error::Result<Self> {
        Ok(Self {
            hw: WorkTxHw::new(hashboard_idx)?,
            tag_manager: tag::TagManager::new(midstate_count),
        })
    }
}

impl CommandIo {
    pub fn init(&mut self) -> error::Result<()> {
        self.hw.init()?;
        Ok(())
    }

    pub fn new(hashboard_idx: usize) -> error::Result<Self> {
        Ok(Self {
            hw: CommandHw::new(hashboard_idx)?,
        })
    }
}

impl ConfigIo {
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

    pub fn init(&mut self) -> error::Result<()> {
        self.hw.init()?;
        self.check_build_id()?;
        Ok(())
    }

    pub fn new(hashboard_idx: usize, midstate_count: MidstateCount) -> error::Result<Self> {
        Ok(Self {
            hw: ConfigHw::new(hashboard_idx)?,
            midstate_count,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    /// Index of chain for testing (must exist and be defined in DTS)
    const TEST_CHAIN_INDEX: usize = 8;

    /// Test that we are able to construct HChainFifo instance
    #[test]
    fn test_fifo_construction() {
        let _io = ConfigIo::new(TEST_CHAIN_INDEX, MidstateCount::new(1)).expect("ConfigIo failed");
        let _io = CommandIo::new(TEST_CHAIN_INDEX).expect("CommandIo failed");
        let _io = WorkRxIo::new(TEST_CHAIN_INDEX, MidstateCount::new(1)).expect("WorkRxIo failed");
        let _io = WorkTxIo::new(TEST_CHAIN_INDEX, MidstateCount::new(4)).expect("WorkTxIo failed");
    }
}
