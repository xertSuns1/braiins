/// This module provides thin API to access memory-mapped FPGA registers
/// and associated interrupts.
/// Exports FIFO management/send/receive and register access.
use crate::error::{self, ErrorKind};
use failure::ResultExt;
use std::mem::size_of;
use std::time::Duration;

use s9_io::hchainio0;

#[cfg(not(feature = "hctl_polling"))]
mod fifo_irq;
#[cfg(feature = "hctl_polling")]
mod fifo_poll;

/// How long to wait for RX interrupt
const FIFO_READ_TIMEOUT: Duration = Duration::from_millis(5);
const WORK_ID_OFFSET: usize = 8;

#[cfg(feature = "hctl_polling")]
pub struct HChainFifo<'a> {
    // the purpose of _hash_chain_map is to keep mmap()-ed memory alive
    _hash_chain_map: uio::UioMapping,
    pub hash_chain_io: &'a hchainio0::RegisterBlock,
    midstate_count_bits: u8,
}

#[cfg(not(feature = "hctl_polling"))]
pub struct HChainFifo<'a> {
    // the purpose of _hash_chain_map is to keep mmap()-ed memory alive
    _hash_chain_map: uio::UioMapping,
    pub hash_chain_io: &'a hchainio0::RegisterBlock,
    work_tx_irq: uio::UioDevice,
    work_rx_irq: uio::UioDevice,
    cmd_rx_irq: uio::UioDevice,
    midstate_count_bits: u8,
}

fn open_ip_core_uio(
    hashboard_idx: usize,
    uio_type: &'static str,
) -> error::Result<(uio::UioDevice, String)> {
    let uio_name = format!("chain{}-{}", hashboard_idx - 1, uio_type);
    Ok((uio::UioDevice::open_by_name(&uio_name)?, uio_name))
}

/// Performs memory mapping of IP core's register block
/// * `hashboard_idx` is the number of chain (numbering must match in device-tree)
fn mmap(hashboard_idx: usize) -> error::Result<uio::UioMapping> {
    let (uio, uio_name) = open_ip_core_uio(hashboard_idx, "mem")?;
    let map = uio
        .map_mapping(0)
        .with_context(|_| ErrorKind::UioDevice(uio_name, "cannot map uio device".to_string()))?;
    Ok(map)
}

/// Performs IRQ mapping of IP core's block
fn map_irq(hashboard_idx: usize, irq_type: &'static str) -> error::Result<uio::UioDevice> {
    let (uio, _uio_name) = open_ip_core_uio(hashboard_idx, irq_type)?;
    Ok(uio)
}

fn u256_as_u32_slice(src: &uint::U256) -> &[u32] {
    unsafe {
        core::slice::from_raw_parts(
            src.0.as_ptr() as *const u32,
            size_of::<uint::U256>() / size_of::<u32>(),
        )
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

    pub fn send_work(
        &mut self,
        work: &crate::hal::MiningWork,
        work_id: u32,
    ) -> Result<u32, failure::Error> {
        self.write_to_work_tx_fifo(work_id)?;
        self.write_to_work_tx_fifo(work.nbits)?;
        self.write_to_work_tx_fifo(work.ntime)?;
        self.write_to_work_tx_fifo(work.merkel_root_lsw)?;

        for midstate in work.midstates.iter() {
            let midstate = u256_as_u32_slice(&midstate);
            // Chip expects the midstate in reverse word order
            for midstate_word in midstate.iter().rev() {
                self.write_to_work_tx_fifo(*midstate_word)?;
            }
        }
        Ok(work_id)
    }

    #[inline]
    fn get_midstate_idx_from_solution_id(&self, solution_id: u32) -> usize {
        ((solution_id >> WORK_ID_OFFSET) & ((1u32 << self.midstate_count_bits) - 1)) as usize
    }

    pub fn recv_solution(
        &mut self,
    ) -> Result<Option<crate::hal::MiningWorkSolution>, failure::Error> {
        let nonce; // = self.fifo.read_from_work_rx_fifo()?;
                   // TODO: to be refactored once we have asynchronous handling in place
                   // fetch command response from IP core's fifo
        match self.read_from_work_rx_fifo()? {
            None => return Ok(None),
            Some(resp_word) => nonce = resp_word,
        }

        let word2;
        match self.read_from_work_rx_fifo()? {
            None => {
                return Err(ErrorKind::Fifo(
                    error::Fifo::TimedOut,
                    "work RX fifo empty".to_string(),
                ))?;
            }
            Some(resp_word) => word2 = resp_word,
        }

        let solution = crate::hal::MiningWorkSolution {
            nonce,
            // this hardware doesn't do any nTime rolling, keep it @ None
            ntime: None,
            midstate_idx: self.get_midstate_idx_from_solution_id(word2),
            // leave the result ID as-is so that we can extract solution index etc later.
            solution_id: word2 & 0xffffffu32,
        };

        Ok(Some(solution))
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
    /// This is to check that the UioMapping Drop trait is working: Drop
    /// does perform unmap which drops the Uio fd lock.
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
        let irq = map_irq(8, "work-tx").unwrap();
        irq.irq_enable().unwrap();
        let res = irq.irq_wait_timeout(FIFO_READ_TIMEOUT);
        assert!(res.unwrap().is_some(), "expected interrupt");
    }

    /// Test that we get timeout when waiting for IRQ.
    /// Test it on empty rx queue (IRQ always deasserted).
    #[test]
    fn test_get_irq_timeout() {
        let irq = map_irq(8, "work-rx").unwrap();
        irq.irq_enable().unwrap();
        let res = irq.irq_wait_timeout(FIFO_READ_TIMEOUT);
        assert!(res.unwrap().is_none(), "expected timeout");
    }
}
