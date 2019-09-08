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

/// This module provides thin API to access memory-mapped FPGA registers
/// and associated interrupts.
/// Exports FIFO management/send/receive and register access.
use std::marker::PhantomData;
use std::ops;
use std::time::Duration;

use ii_fpga_io_am1_s9::hchainio0;

use super::error::{self, ErrorKind};
use crate::hal::{self, s9};
use crate::work;
use failure::ResultExt;

/// How long to wait for RX interrupt
const FIFO_READ_TIMEOUT: Duration = Duration::from_millis(5);

unsafe impl Send for HChainFifo {}
unsafe impl Sync for HChainFifo {}

/// Reference-like type holding a memory map created using UioMapping
/// Used to hold a memory mapping of IP core's register block
pub struct Mmap<T = u8> {
    map: uio_async::UioMapping,
    _marker: PhantomData<*const T>,
}

impl<T> Mmap<T> {
    /// Create a new memory mapping
    /// * `hashboard_idx` is the number of chain (numbering must match in device-tree)
    ///
    /// Marked `unsafe` because we can't check whether `T` is sized correctly and makes sense
    unsafe fn new(hashboard_idx: usize) -> error::Result<Self> {
        let (uio, uio_name) = open_ip_core_uio(hashboard_idx, "mem")?;
        let map = uio.map_mapping(0).with_context(|_| {
            ErrorKind::UioDevice(uio_name, "cannot map uio device".to_string())
        })?;

        Ok(Self {
            map,
            _marker: PhantomData,
        })
    }
}

impl<T> ops::Deref for Mmap<T> {
    type Target = T;

    fn deref(&self) -> &T {
        let ptr = self.map.ptr as *const T;
        unsafe { &*ptr }
    }
}

/// How big is FIFO queue? (in u32 words)
const FIFO_WORK_TX_SIZE: u32 = 2048;
/// How big is the absolute biggest "work"? (again, in u32 words)
const FIFO_WORK_MAX_SIZE: u32 = 200;
/// Threshold for number of entries in FIFO queue under which we recon we could
/// fit one more work.
const FIFO_WORK_TX_THRESHOLD: u32 = FIFO_WORK_TX_SIZE - FIFO_WORK_MAX_SIZE;
/// What bitstream version do we expect
const EXPECTED_BITSTREAM_BUILD_ID: u32 = 0x5D5E7158;

pub struct HChainFifo {
    pub hash_chain_io: Mmap<hchainio0::RegisterBlock>,
    pub midstate_count: s9::MidstateCount,
    work_tx_irq: uio_async::UioDevice,
    work_rx_irq: uio_async::UioDevice,
    cmd_rx_irq: uio_async::UioDevice,
}

fn open_ip_core_uio(
    hashboard_idx: usize,
    uio_type: &'static str,
) -> error::Result<(uio_async::UioDevice, String)> {
    let uio_name = format!("chain{}-{}", hashboard_idx - 1, uio_type);
    Ok((uio_async::UioDevice::open_by_name(&uio_name)?, uio_name))
}

/// Performs IRQ mapping of IP core's block
fn map_irq(hashboard_idx: usize, irq_type: &'static str) -> error::Result<uio_async::UioDevice> {
    let (uio, _uio_name) = open_ip_core_uio(hashboard_idx, irq_type)?;
    Ok(uio)
}

/// This is common implementation
impl HChainFifo {
    #[inline]
    pub fn is_work_tx_fifo_full(&self) -> bool {
        self.hash_chain_io.stat_reg.read().work_tx_full().bit()
    }

    #[inline]
    pub fn is_work_rx_fifo_empty(&self) -> bool {
        self.hash_chain_io.stat_reg.read().work_rx_empty().bit()
    }

    #[inline]
    pub fn is_cmd_rx_fifo_empty(&self) -> bool {
        self.hash_chain_io.stat_reg.read().cmd_rx_empty().bit()
    }

    #[inline]
    pub fn has_work_tx_space_for_one_job(&self) -> bool {
        self.hash_chain_io.stat_reg.read().irq_pend_work_tx().bit()
    }

    /// Wait fro command FIFO to become empty.
    /// Uses polling.
    pub fn wait_cmd_tx_fifo_empty(&self) {
        // TODO busy waiting has to be replaced once asynchronous processing is in place
        // jho: Not really, there's no IRQ for cmd tx fifo becomming "empty". The best we
        // can do is run this in a separate thread with timeout polling.
        // But we usually want to wait for cmd to be empty before we issue other commands,
        // so it's not really worth it to pursue asynchronicity vehemently in this case.
        while !self.hash_chain_io.stat_reg.read().cmd_tx_empty().bit() {}
    }

    pub fn enable_ip_core(&self) {
        self.hash_chain_io
            .ctrl_reg
            .modify(|_, w| w.enable().bit(true));
    }

    pub fn disable_ip_core(&self) {
        self.hash_chain_io
            .ctrl_reg
            .modify(|_, w| w.enable().bit(false));
    }

    pub fn set_ip_core_work_time(&self, work_time: u32) {
        self.hash_chain_io
            .work_time
            .write(|w| unsafe { w.bits(work_time) });
    }

    pub fn set_baud_clock_div(&self, baud_clock_div: u32) {
        self.hash_chain_io
            .baud_reg
            .write(|w| unsafe { w.bits(baud_clock_div) });
    }

    pub fn set_ip_core_midstate_count(&self, value: hchainio0::ctrl_reg::MIDSTATE_CNT_A) {
        self.hash_chain_io
            .ctrl_reg
            .modify(|_, w| w.midstate_cnt().variant(value));
    }

    pub fn send_work(
        &mut self,
        work: &work::Assignment,
        work_id: u32,
    ) -> Result<u32, failure::Error> {
        let hw_midstate_count = self.midstate_count.to_count();
        let expected_midstate_count = work.midstates.len();
        assert_eq!(
            expected_midstate_count, hw_midstate_count,
            "Expected {} midstates, but S9 is configured for {} midstates!",
            expected_midstate_count, hw_midstate_count,
        );

        self.write_to_work_tx_fifo(work_id.to_le())?;
        self.write_to_work_tx_fifo(work.bits().to_le())?;
        self.write_to_work_tx_fifo(work.ntime.to_le())?;
        self.write_to_work_tx_fifo(work.merkle_root_tail().to_le())?;

        for mid in work.midstates.iter() {
            for midstate_word in mid.state.words::<u32>().rev() {
                self.write_to_work_tx_fifo(midstate_word.to_be())?;
            }
        }
        Ok(work_id)
    }

    pub async fn recv_solution(
        mut self,
    ) -> Result<(Self, hal::MiningWorkSolution), failure::Error> {
        let nonce = await!(self.async_read_from_work_rx_fifo())?;
        let word2 = await!(self.async_read_from_work_rx_fifo())?;
        let solution_id = s9::SolutionId::from_reg(word2, self.midstate_count);

        let solution = hal::MiningWorkSolution {
            nonce,
            ntime: None,
            midstate_idx: solution_id.midstate_idx,
            solution_idx: solution_id.solution_idx,
            solution_id: word2,
        };

        Ok((self, solution))
    }

    /// Return the value of last work ID send to ASICs
    #[inline]
    pub fn get_last_work_id(&mut self) -> u32 {
        self.hash_chain_io.last_work_id.read().bits()
    }

    /// Return build id (unix timestamp) of s9-io FPGA bitstream
    #[inline]
    pub fn get_build_id(&mut self) -> u32 {
        self.hash_chain_io.build_id.read().bits()
    }

    /// Try to write work item to work TX FIFO.
    /// Performs blocking write without timeout. Uses IRQ.
    /// The idea is that you don't call this function until you are sure you
    /// can fit in all the entries you want - for example
    /// `hash_work_tx_space_for_one_job`.
    #[inline]
    pub fn write_to_work_tx_fifo(&mut self, item: u32) -> error::Result<()> {
        let cond = || !self.is_work_tx_fifo_full();
        self.work_tx_irq.irq_wait_cond(cond, None)?;
        self.hash_chain_io
            .work_tx_fifo
            .write(|w| unsafe { w.bits(item) });
        Ok(())
    }

    /// Try to read from work rx fifo.
    /// Performs blocking read with timeout. Uses IRQ.
    #[inline]
    pub fn read_from_work_rx_fifo(&mut self) -> error::Result<Option<u32>> {
        let cond = || !self.is_work_rx_fifo_empty();
        let got_irq = self
            .work_rx_irq
            .irq_wait_cond(cond, Some(FIFO_READ_TIMEOUT))?;
        Ok(got_irq.and_then(|_| Some(self.hash_chain_io.work_rx_fifo.read().bits())))
    }

    pub async fn async_read_from_work_rx_fifo(&mut self) -> error::Result<u32> {
        let cond = || !self.is_work_rx_fifo_empty();
        await!(self.work_rx_irq.async_irq_wait_cond(cond))?;
        Ok(self.hash_chain_io.work_rx_fifo.read().bits())
    }

    /// Try to write command to cmd tx fifo.
    /// Performs blocking write without timeout. Uses polling.
    /// TODO get rid of busy waiting, prepare for non-blocking API
    #[inline]
    pub fn write_to_cmd_tx_fifo(&self, item: u32) {
        while self.hash_chain_io.stat_reg.read().cmd_tx_full().bit() {}
        self.hash_chain_io
            .cmd_tx_fifo
            .write(|w| unsafe { w.bits(item) });
    }

    /// Try to read command from cmd rx fifo.
    /// Performs blocking read with timeout. Uses IRQ.
    #[inline]
    pub fn read_from_cmd_rx_fifo(&mut self) -> error::Result<Option<u32>> {
        let cond = || !self.is_cmd_rx_fifo_empty();
        let got_irq = self
            .cmd_rx_irq
            .irq_wait_cond(cond, Some(FIFO_READ_TIMEOUT))?;
        Ok(got_irq.and_then(|_| Some(self.hash_chain_io.cmd_rx_fifo.read().bits())))
    }

    pub async fn async_read_from_cmd_rx_fifo(&mut self) -> error::Result<u32> {
        let cond = || !self.is_cmd_rx_fifo_empty();
        await!(self.cmd_rx_irq.async_irq_wait_cond(cond))?;
        Ok(self.hash_chain_io.cmd_rx_fifo.read().bits())
    }

    pub async fn async_wait_for_work_tx_room(&self) -> error::Result<()> {
        let cond = || self.has_work_tx_space_for_one_job();
        await!(self.work_tx_irq.async_irq_wait_cond(cond))?;
        Ok(())
    }

    fn check_build_id(&mut self) -> error::Result<()> {
        let build_id = self.get_build_id();
        if build_id != EXPECTED_BITSTREAM_BUILD_ID {
            Err(ErrorKind::UnexpectedVersion(
                "s9-io bitstream".to_string(),
                format!("0x{:08x}", build_id),
                format!("0x{:08x}", EXPECTED_BITSTREAM_BUILD_ID),
            ))?
        }
        Ok(())
    }
    pub fn init(&mut self) -> error::Result<()> {
        // Check if we run the right version of bitstream
        self.check_build_id()?;

        // Set threshold for work TX so that there's space for
        // at least one job.
        self.hash_chain_io
            .irq_fifo_thr
            .write(|w| unsafe { w.bits(FIFO_WORK_TX_THRESHOLD) });
        // Reset FIFOs
        self.hash_chain_io.ctrl_reg.modify(|_, w| {
            w.rst_cmd_rx_fifo()
                .set_bit()
                .rst_cmd_tx_fifo()
                .set_bit()
                .rst_work_rx_fifo()
                .set_bit()
                .rst_work_tx_fifo()
                .set_bit()
        });
        // enable IRQ_WORK_TX interrupt
        self.hash_chain_io
            .ctrl_reg
            .modify(|_, w| w.irq_en_work_tx().set_bit());
        // enable IRQ_WORK_RX interrupt
        self.hash_chain_io
            .ctrl_reg
            .modify(|_, w| w.irq_en_work_rx().set_bit());
        // enable IRQ_CMD_RX interrupt
        self.hash_chain_io
            .ctrl_reg
            .modify(|_, w| w.irq_en_cmd_rx().set_bit());

        Ok(())
    }

    pub fn new(hashboard_idx: usize, midstate_count: s9::MidstateCount) -> error::Result<Self> {
        let hash_chain_io = unsafe { Mmap::new(hashboard_idx)? };

        let fifo = Self {
            hash_chain_io,
            midstate_count,
            work_tx_irq: map_irq(hashboard_idx, "work-tx")?,
            work_rx_irq: map_irq(hashboard_idx, "work-rx")?,
            cmd_rx_irq: map_irq(hashboard_idx, "cmd-rx")?,
        };
        Ok(fifo)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Index of chain for testing (must exist and be defined in DTS)
    const TEST_CHAIN_INDEX: usize = 8;

    /// Try opening UIO device.
    /// This test needs properly configured UIO devices for hash-chain 8 in
    /// device-tree so that we have something to open.
    #[test]
    fn test_lookup_uio() {
        let name = String::from("chain7-mem");
        uio_async::UioDevice::open_by_name(&name).unwrap();
    }

    /// Try opening non-existent UIO device.
    #[test]
    fn test_lookup_uio_notfound() {
        let name = String::from("chain7-nonsense");
        let uio = uio_async::UioDevice::open_by_name(&name);
        assert!(
            uio.is_err(),
            "Found UIO device {} that shouldn't really be there"
        );
    }

    /// Try mapping memory from UIO device.
    #[test]
    fn test_map_uio() {
        unsafe {
            let _m: Mmap<u8> = Mmap::new(TEST_CHAIN_INDEX).unwrap();
        }
    }

    /// Try to map memory twice.
    /// This is to check that the UioMapping Drop trait is working: Drop
    /// does perform unmap which drops the Uio fd lock.
    #[test]
    fn test_map_uio_twice_checklock() {
        unsafe {
            let _m: Mmap<u8> = Mmap::new(TEST_CHAIN_INDEX).unwrap();
            let _m: Mmap<u8> = Mmap::new(TEST_CHAIN_INDEX).unwrap();
        }
    }

    /// Test that we are able to construct HChainFifo instance
    #[test]
    fn test_fifo_construction() {
        let _fifo = HChainFifo::new(TEST_CHAIN_INDEX, s9::MidstateCount::new(1))
            .expect("fifo construction failed");
    }

    /// Try to map IRQ.
    #[test]
    fn test_map_irq() {
        map_irq(TEST_CHAIN_INDEX, "cmd-rx").unwrap();
    }

    /// Test that we get IRQ.
    /// Test it on empty tx queue (IRQ always asserted).
    #[test]
    fn test_get_irq() {
        let irq = map_irq(TEST_CHAIN_INDEX, "work-tx").unwrap();
        irq.irq_enable().unwrap();
        let res = irq.irq_wait_timeout(FIFO_READ_TIMEOUT);
        assert!(res.unwrap().is_some(), "expected interrupt");
    }

    /// Test that we get timeout when waiting for IRQ.
    /// Test it on empty rx queue (IRQ always deasserted).
    #[test]
    fn test_get_irq_timeout() {
        let mut fifo = HChainFifo::new(TEST_CHAIN_INDEX, s9::MidstateCount::new(1))
            .expect("fifo construction failed");
        // fifo initialization flushes all received responses
        fifo.init().expect("fifo initialization failed");
        drop(fifo);
        // work rx fifo now shouldn't get any interrupts (it's empty)
        let irq = map_irq(TEST_CHAIN_INDEX, "work-rx").unwrap();
        irq.irq_enable().unwrap();
        let res = irq.irq_wait_timeout(FIFO_READ_TIMEOUT);
        assert!(res.unwrap().is_none(), "expected timeout");
    }
}
