/// This module provides thin API to access memory-mapped FPGA registers
/// and associated interrupts.
/// Exports FIFO management/send/receive and register access.
use crate::error::{self, ErrorKind};
use failure::ResultExt;
use std::time::Duration;

use s9_io::hchainio0;

/// How big is FIFO queue? (in u32 words)
const FIFO_WORK_TX_SIZE: u32 = 2048;
/// How big is the absolute biggest "work"? (again, in u32 words)
const FIFO_WORK_MAX_SIZE: u32 = 200;
/// Threshold for number of entries in FIFO queue under which we recon we could
/// fit one more work.
const FIFO_WORK_TX_THRESHOLD: u32 = FIFO_WORK_TX_SIZE - FIFO_WORK_MAX_SIZE;
/// How long to wait for RX interrupt
const FIFO_READ_TIMEOUT: Duration = Duration::from_millis(5);

#[cfg(feature = "hctl_polling")]
pub struct HChainFifo<'a> {
    pub hash_chain_io: &'a hchainio0::RegisterBlock,
}

#[cfg(not(feature = "hctl_polling"))]
pub struct HChainFifo<'a> {
    pub hash_chain_io: &'a hchainio0::RegisterBlock,
    work_tx_irq: uio::UioDevice,
    work_rx_irq: uio::UioDevice,
    cmd_rx_irq: uio::UioDevice,
}

/// Performs memory mapping of IP core's register block
/// * `hashboard_idx` is the number of chain (numbering must match in device-tree)
fn mmap(hashboard_idx: usize) -> error::Result<*const hchainio0::RegisterBlock> {
    let uio_name = format!("chain{}-mem", hashboard_idx - 1);
    let uio = uio::UioDevice::open_by_name(&uio_name)?;
    let mem = uio
        .map_mapping(0)
        .with_context(|_| ErrorKind::UioDevice(uio_name, "cannot map uio device".to_string()))?;
    Ok(mem as *const hchainio0::RegisterBlock)
}

/// Performs IRQ mapping of IP core's block
fn map_irq(hashboard_idx: usize, irq_type: &'static str) -> error::Result<uio::UioDevice> {
    let uio_name = format!("chain{}-{}", hashboard_idx - 1, irq_type);
    let uio = uio::UioDevice::open_by_name(&uio_name)?;
    Ok(uio)
}

/// This is implementation for IRQs
#[cfg(feature = "hctl_polling")]
impl<'a> HChainFifo<'a> {
    /// TODO: implement error handling/make interface ready for ASYNC execution
    /// Writes single word into a TX fifo
    #[inline]
    pub fn write_to_work_tx_fifo(&self, item: u32) -> error::Result<()> {
        while self.is_work_tx_fifo_full() {}
        self.hash_chain_io
            .work_tx_fifo
            .write(|w| unsafe { w.bits(item) });
        Ok(())
    }

    #[inline]
    pub fn read_from_work_rx_fifo(&self) -> error::Result<Option<u32>> {
        // TODO temporary workaround until we have asynchronous handling - wait 5 ms if the FIFO
        // is empty
        if self.hash_chain_io.stat_reg.read().work_rx_empty().bit() {
            thread::sleep(FIFO_READ_TIMEOUT);
        }
        if self.hash_chain_io.stat_reg.read().work_rx_empty().bit() {
            return Ok(None);
        }
        Ok(Some(self.hash_chain_io.work_rx_fifo.read().bits()))
    }

    /// TODO get rid of busy waiting, prepare for non-blocking API
    #[inline]
    pub fn write_to_cmd_tx_fifo(&self, item: u32) {
        while self.hash_chain_io.stat_reg.read().cmd_tx_full().bit() {}
        self.hash_chain_io
            .cmd_tx_fifo
            .write(|w| unsafe { w.bits(item) });
    }

    #[inline]
    pub fn read_from_cmd_rx_fifo(&self) -> error::Result<Option<u32>> {
        // TODO temporary workaround until we have asynchronous handling - wait 5 ms if the FIFO
        // is empty
        if self.hash_chain_io.stat_reg.read().cmd_rx_empty().bit() {
            thread::sleep(FIFO_READ_TIMEOUT);
        }
        if self.hash_chain_io.stat_reg.read().cmd_rx_empty().bit() {
            return Ok(None);
        }
        Ok(Some(self.hash_chain_io.cmd_rx_fifo.read().bits()))
    }

    #[inline]
    pub fn has_work_tx_space_for_one_job(&self) -> bool {
        self.hash_chain_io.stat_reg.read().irq_pend_work_tx().bit()
    }

    pub fn new(hashboard_idx: usize) -> error::Result<Self> {
        let hash_chain_io = mmap(hashboard_idx)?;
        let hash_chain_io = unsafe { &*hash_chain_io };
        Ok(Self { hash_chain_io })
    }
}

/// This is implementation for IRQs
#[cfg(not(feature = "hctl_polling"))]
impl<'a> HChainFifo<'a> {
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

    #[inline]
    pub fn has_work_tx_space_for_one_job(&self) -> bool {
        self.hash_chain_io.stat_reg.read().irq_pend_work_tx().bit()
    }

    fn init_irqs(&mut self) {
        // Set threshold for work TX so that there's space for
        // at least one job.
        self.hash_chain_io
            .irq_fifo_thr
            .write(|w| unsafe { w.bits(FIFO_WORK_TX_THRESHOLD) });
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
    }

    pub fn new(hashboard_idx: usize) -> error::Result<Self> {
        let hash_chain_io = mmap(hashboard_idx)?;
        let hash_chain_io = unsafe { &*hash_chain_io };

        let mut fifo = Self {
            hash_chain_io: hash_chain_io,
            work_tx_irq: map_irq(hashboard_idx, "work-tx")?,
            work_rx_irq: map_irq(hashboard_idx, "work-rx")?,
            cmd_rx_irq: map_irq(hashboard_idx, "cmd-rx")?,
        };
        fifo.init_irqs();
        Ok(fifo)
    }
}

/// This is common implementation
impl<'a> HChainFifo<'a> {
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

    pub fn set_ip_core_midstate_count(&self, count: u8) {
        self.hash_chain_io
            .ctrl_reg
            .modify(|_, w| unsafe { w.midstate_cnt().bits(count) });
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::fs;
    use std::io;

    /// Try opening UIO device.
    /// This test needs properly configured UIO devices for hash-chain 8 in
    /// device-tree so that we have something to open.
    #[test]
    fn test_lookup_uio() {
        let name = String::from("chain7-mem");
        uio::UioDevice::open_by_name(&name).unwrap();
    }

    /// Try opening non-existent UIO device.
    #[test]
    fn test_lookup_uio_notfound() {
        let name = String::from("chain7-nonsense");
        let uio = uio::UioDevice::open_by_name(&name);
        assert!(
            uio.is_err(),
            "Found UIO device {} that shouldn't really be there"
        );
    }

    /// Try mapping memory from UIO device.
    #[test]
    fn test_map_uio() {
        mmap(8).unwrap();
    }

    /// Try to map memory twice.
    /// This is to check that the UIO locking/unlocking (in Drop) is working.
    #[test]
    fn test_map_uio_twice_checklock() {
        mmap(8).unwrap();
        mmap(8).unwrap();
    }

    /// Try to map IRQ.
    #[test]
    fn test_map_irq() {
        map_irq(8, "cmd-rx").unwrap();
    }

    /// Test that we get IRQ.
    /// Test it on empty tx queue (IRQ always asserted).
    #[test]
    fn test_get_irq() {
        let mut irq = map_irq(8, "work-tx").unwrap();
        irq.irq_enable().unwrap();
        let res = irq.irq_wait_timeout(FIFO_READ_TIMEOUT);
        res.expect("immediate interrupt expected");
    }

    /// Test that we get timeout when waiting for IRQ.
    /// Test it on empty rx queue (IRQ always deasserted).
    #[test]
    fn test_get_irq_timeout() {
        let mut irq = map_irq(8, "work-rx").unwrap();
        irq.irq_enable().unwrap();
        let res = irq.irq_wait_timeout(FIFO_READ_TIMEOUT);
        assert!(
            res.expect_err("expecting timeout").kind() == io::ErrorKind::TimedOut,
            "expecting timeout error"
        );
    }
}
