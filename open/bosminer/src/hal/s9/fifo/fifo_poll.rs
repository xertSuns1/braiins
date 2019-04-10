use super::*;
use crate::error;
use std::thread;

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
