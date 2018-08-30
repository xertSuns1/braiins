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

use uint;

use self::s9_io::hchainio0;

mod bm1387;

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
                ProtFlags::PROT_READ | ProtFlags::PROT_READ,
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
        })
    }

    // TODO handle error
    pub fn init(&self) {
        // Disable/enable performs reset of the entire FPGA IP core
        self.disable();
        self.enable();

        self.set_baud(115200);
        // TODO consolidate hardcoded constant - calculate time constant based on PLL settings etc.
        self.set_work_time(50000);
        self.set_midstate_count(hchainio0::ctrl_reg::MIDSTATE_CNTW::ONE);
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
    /// TODO: implement error handling
    /// Writes single word into a TX fifo
    fn write_to_work_tx_fifo(&self, item: u32) -> bool {
        let hash_chain_io = self.hash_chain_ios[0];
        let may_write = !hash_chain_io.stat_reg.read().cmd_tx_full().bit();
        if may_write == true {
            hash_chain_io
                .work_tx_fifo
                .write(|w| unsafe { w.bits(item) });
        }
        return may_write;
    }

    #[inline]
    fn next_work_id(&mut self) -> u32 {
        let retval = self.work_id as u32;
        self.work_id += 1;
        retval
    }
}

impl<'a> super::HardwareCtl for HChainCtl<'a> {
    fn send_work(&self, work: &super::MiningWork) {
        //self.write_to_work_tx_fifo(self.next_work_id());
        self.write_to_work_tx_fifo(work.nbits);
        self.write_to_work_tx_fifo(work.n_time);
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

        h_chain_ctl.init();

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
    }

}
