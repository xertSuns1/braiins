extern crate byteorder;
extern crate libc;
extern crate nix;
extern crate s9_io;

use self::nix::sys::mman::{MapFlags, ProtFlags};

use core;
use std::fs::OpenOptions;
use std::io;
use std::mem::size_of;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
// TODO: remove thread specific components
use std::thread;
use std::time::Duration;

use uint;

use self::byteorder::{ByteOrder, LittleEndian};
use packed_struct::{PackedStruct, PackedStructSlice};
use self::s9_io::hchainio0;

mod bm1387;

/// Timing constants
const INACTIVATE_FROM_CHAIN_DELAY_MS: u64 = 100;

/// Maximum number of chips is limitted by the fact that there is only 8-bit address field and
/// addresses to the chips need to be assigned with step of 4 (e.g. 0, 4, 8, etc.)
const MAX_CHIPS_ON_CHAIN: usize = 64;

/// Hash Chain Controller provides abstraction of the FPGA interface for operating hashing boards.
/// It is the user-space driver for the
///
/// Main responsibilities:
/// - memory mapping of the FPGA control interface
/// - hashing work submission and fetching
///
/// TODO: implement drop trait (results in unmap)
pub struct HChainCtl<'a> {
    hash_chain_ios: [&'a hchainio0::RegisterBlock; 2],
    work_id: u16,
    chip_count: usize,
}

impl<'a> HChainCtl<'a> {
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
    pub fn new() -> Result<Self, io::Error> {
        let hash_chain_io = Self::mmap()?;
        let hash_chain_io = unsafe { &*hash_chain_io };
        Result::Ok(Self {
            hash_chain_ios: [hash_chain_io, hash_chain_io],
            work_id: 0,
            chip_count: 0,
        })
    }

    /// Helper method that initializes the FPGA IP core
    fn ip_core_init(&self) -> Result<(), io::Error> {
        // Disable/enable performs reset of the entire FPGA IP core
        self.disable();
        self.enable();

        self.set_baud(115200);
        // TODO consolidate hardcoded constant - calculate time constant based on PLL settings etc.
        self.set_work_time(50000);
        self.set_midstate_count(hchainio0::ctrl_reg::MIDSTATE_CNTW::ONE);

        Ok(())
    }

    /// Initializes the chips on the chain
    pub fn init(&mut self) -> Result<(), io::Error> {
        self.ip_core_init()?;

        self.enumerate_chips()?;
        println!("Discovered {} chips", self.chip_count);

        // set PLL
        self.set_pll()?;

        // enable hashing chain
        self.configure_hash_chain()?;

        Ok(())
    }

    /// Detects the number of chips on the hashing chain and assigns an address to each chip
    fn enumerate_chips(&mut self) -> Result<(), io::Error> {
        // Enumerate all chips (broadcast read address register request)
        let get_addr_cmd = bm1387::GetStatusCmd::new(0, true, bm1387::GET_ADDRESS_REG).pack();
        self.send_ctl_cmd(&get_addr_cmd);
        self.chip_count = 0;
        while let Ok(addr_reg) = self.recv_ctl_cmd_resp::<bm1387::GetAddressReg>() {
            assert_ne!(
                addr_reg.chip_rev,
                bm1387::ChipRev::Bm1387,
                "Unexpected chip revision"
            );
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
        // Set all chips to be offline before address assignment. This is important so that each
        // chip after initially accepting the address will pass on further addresses down the chain
        let inactivate_from_chain_cmd = bm1387::InactivateFromChainCmd::new().pack();
        // make sure all chips receive inactivation request
        for _ in 0..3 {
            self.send_ctl_cmd(&inactivate_from_chain_cmd);
            thread::sleep(Duration::from_millis(INACTIVATE_FROM_CHAIN_DELAY_MS));
        }

        // Assign address to each chip
        self.for_all_chips(|addr| {
            let cmd = bm1387::SetChipAddressCmd::new(addr);
            self.send_ctl_cmd(&cmd.pack());
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
            self.send_ctl_cmd(&cmd.pack());
            Ok(())
        })
    }

    /// TODO: consolidate hardcoded baudrate to 115200
    fn configure_hash_chain(&self) -> Result<(), io::Error> {
        let ctl_reg = bm1387::MiscCtrlReg {
            not_set_baud: true,
            inv_clock: true,
            baud_div: 26.into(),
            gate_block: true,
            mmen: true,
        };
        let ctl_reg_u32 = ctl_reg.to_u32();
        let cmd = bm1387::SetConfigCmd::new(0, true, bm1387::MISC_CONTROL_REG, ctl_reg_u32);
        self.send_ctl_cmd(&cmd.pack());
        Ok(())
    }

    fn enable(&self) {
        self.hash_chain_ios[0]
            .ctrl_reg
            .write(|w| w.enable().bit(true));
    }

    fn disable(&self) {
        self.hash_chain_ios[0]
            .ctrl_reg
            .write(|w| w.enable().bit(false));
    }

    fn set_work_time(&self, work_time: u32) {
        self.hash_chain_ios[0]
            .work_time
            .write(|w| unsafe { w.bits(work_time) });
    }

    /// TODO make parametric and remove hardcoded baudrate constant
    fn set_baud(&self, _baud: u32) {
        self.hash_chain_ios[0]
            .baud_reg
            .write(|w| unsafe { w.bits(0x1b) });
    }

    fn set_midstate_count(&self, count: s9_io::hchainio0::ctrl_reg::MIDSTATE_CNTW) {
        self.hash_chain_ios[0]
            .ctrl_reg
            .write(|w| w.midstate_cnt().variant(count));
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
    fn next_work_id(&mut self) -> u32 {
        let retval = self.work_id as u32;
        self.work_id += 1;
        retval
    }

    #[inline]
    /// TODO: implement error handling/make interface ready for ASYNC execution
    /// Writes single word into a TX fifo
    fn write_to_work_tx_fifo(&self, item: u32) {
        let hash_chain_io = self.hash_chain_ios[0];
        while !hash_chain_io.stat_reg.read().work_tx_full().bit() {}
        hash_chain_io
            .work_tx_fifo
            .write(|w| unsafe { w.bits(item) });
    }

    #[inline]
    /// TODO get rid of busy waiting, prepare for non-blocking API
    fn read_from_work_rx_fifo(&self) -> u32 {
        let hash_chain_io = self.hash_chain_ios[0];
        while hash_chain_io.stat_reg.read().work_rx_empty().bit() {}
        hash_chain_io.work_rx_fifo.read().bits()
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
        if hash_chain_io.stat_reg.read().cmd_rx_empty().bit() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Command RX fifo empty",
            ));
        }
        Ok(hash_chain_io.cmd_rx_fifo.read().bits())
    }

    /// Serializes command into 32-bit words and submits it to the command TX FIFO
    ///
    fn send_ctl_cmd(&self, cmd: &[u8]) {
        // invariant required by the IP core
        assert_eq!(
            cmd.len() & 0x3,
            0,
            "Control command length not aligned to 4 byte boundary!"
        );
        for chunk in cmd.chunks(4) {
            self.write_to_cmd_tx_fifo(LittleEndian::read_u32(chunk));
        }
    }

    /// # TODO
    ///
    /// # Errors
    ///
    fn recv_ctl_cmd_resp<T: PackedStructSlice>(&self) -> Result<T, io::Error> {
        let mut cmd_resp = [0u8; 8];
        // fetch command response from IP core's fifo
        for cmd in cmd_resp.chunks_mut(4) {
            let resp_word = self.read_from_cmd_rx_fifo()?;
            LittleEndian::write_u32(cmd, resp_word);
        }
        // build the response instance
        T::unpack_from_slice(&cmd_resp).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "Control command unpacking error! {:?} {:#04x?}",
                    e, cmd_resp
                ),
            )
        })
    }
}

impl<'a> super::HardwareCtl for HChainCtl<'a> {
    fn send_work(&mut self, work: &super::MiningWork) {
        let next_work_id = self.next_work_id();
        self.write_to_work_tx_fifo(next_work_id);
        self.write_to_work_tx_fifo(work.nbits);
        self.write_to_work_tx_fifo(work.ntime);
        self.write_to_work_tx_fifo(work.merkel_root_lsw);

        for midstate in work.midstates {
            let midstate = HChainCtl::u256_as_u32_slice(&midstate);
            // Chip expects the midstate in reverse word order
            for midstate_word in midstate.iter().rev() {
                self.write_to_work_tx_fifo(*midstate_word);
            }
        }
    }
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
        let h_chain_ctl = HChainCtl::new();
        match h_chain_ctl {
            Ok(_) => assert!(true),
            Err(e) => assert!(false, "Failed to instantiate hash chain, error: {}", e),
        }
    }

    #[test]
    fn test_hchain_ctl_init() {
        let h_chain_ctl = HChainCtl::new().unwrap();

        assert!(
            h_chain_ctl.ip_core_init().is_ok(),
            "Failed to initialize IP core"
        );

        // verify sane register values
        assert_eq!(
            h_chain_ctl.hash_chain_ios[0].work_time.read().bits(),
            50000,
            "Unexpected work time value"
        );
        assert_eq!(
            h_chain_ctl.hash_chain_ios[0].baud_reg.read().bits(),
            0x1b,
            "Unexpected baudrate register value"
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

}
