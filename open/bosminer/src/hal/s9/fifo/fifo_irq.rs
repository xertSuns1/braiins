use super::*;

/// How big is FIFO queue? (in u32 words)
const FIFO_WORK_TX_SIZE: u32 = 2048;
/// How big is the absolute biggest "work"? (again, in u32 words)
const FIFO_WORK_MAX_SIZE: u32 = 200;
/// Threshold for number of entries in FIFO queue under which we recon we could
/// fit one more work.
const FIFO_WORK_TX_THRESHOLD: u32 = FIFO_WORK_TX_SIZE - FIFO_WORK_MAX_SIZE;
/// What bitstream version do we expect
const EXPECTED_BITSTREAM_BUILD_ID: u32 = 0x5D5E7158;

impl HChainFifo {
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

    pub fn new(hashboard_idx: usize) -> error::Result<Self> {
        let hash_chain_io = unsafe { Mmap::new(hashboard_idx)? };

        let fifo = Self {
            hash_chain_io,
            midstate_count: None,
            work_tx_irq: map_irq(hashboard_idx, "work-tx")?,
            work_rx_irq: map_irq(hashboard_idx, "work-rx")?,
            cmd_rx_irq: map_irq(hashboard_idx, "cmd-rx")?,
        };
        Ok(fifo)
    }
}
