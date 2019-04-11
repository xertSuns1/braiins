use super::*;
use crate::error;

/// How big is FIFO queue? (in u32 words)
const FIFO_WORK_TX_SIZE: u32 = 2048;
/// How big is the absolute biggest "work"? (again, in u32 words)
const FIFO_WORK_MAX_SIZE: u32 = 200;
/// Threshold for number of entries in FIFO queue under which we recon we could
/// fit one more work.
const FIFO_WORK_TX_THRESHOLD: u32 = FIFO_WORK_TX_SIZE - FIFO_WORK_MAX_SIZE;

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
        let hash_chain_map = mmap(hashboard_idx)?;
        let hash_chain_io = hash_chain_map.ptr as *const hchainio0::RegisterBlock;
        let hash_chain_io = unsafe { &*hash_chain_io };

        let mut fifo = Self {
            _hash_chain_map: hash_chain_map,
            hash_chain_io,
            work_tx_irq: map_irq(hashboard_idx, "work-tx")?,
            work_rx_irq: map_irq(hashboard_idx, "work-rx")?,
            cmd_rx_irq: map_irq(hashboard_idx, "cmd-rx")?,
        };
        fifo.init_irqs();
        Ok(fifo)
    }
}
