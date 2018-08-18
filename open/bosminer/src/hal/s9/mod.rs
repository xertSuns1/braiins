extern crate libc;
extern crate nix;
extern crate s9_io;

use self::nix::sys::mman::{MapFlags, ProtFlags};
use std::fs::OpenOptions;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;

mod bm1387;

/// Hash Chain Controller provides abstraction of the FPGA interface for operating hashing boards.
/// It is the user-space driver for the
///
/// Main responsibilities:
/// - memory mapping of the FPGA control interface
/// - hashing work submission and fetching
pub struct HChainCtl<'a> {
    hash_chain_ios: [&'a s9_io::hchainio0::RegisterBlock; 2],
}

impl<'a> HChainCtl<'a> {
    fn mmap() -> Result<*const s9_io::hchainio0::RegisterBlock, io::Error> {
        let mem_file = //File::open(path)?;
            OpenOptions::new().read(true)
                //.custom_flags(libc::O_RDWR | libc::O_SYNC | libc::O_LARGEFILE)
                .open("/dev/mem")?;

        let mmap = unsafe {
            nix::sys::mman::mmap(
                0 as *mut libc::c_void,
                4096,
                ProtFlags::PROT_READ,
                MapFlags::MAP_SHARED,
                mem_file.as_raw_fd(),
                s9_io::HCHAINIO0::ptr() as libc::off_t,
            )
        };
        mmap.map(|addr| addr as *const s9_io::hchainio0::RegisterBlock)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("mmap error! {:?}", e)))
    }
    pub fn new() -> Result<Self, io::Error> {
        let hash_chain_io = Self::mmap()?;
        let hash_chain_io = unsafe { &*hash_chain_io };
        Result::Ok(Self {
            hash_chain_ios: [hash_chain_io, hash_chain_io],
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_hchain_ctl_instance() {
        let h_chain_ctl = HChainCtl::new();
        match h_chain_ctl {
            Ok(_) => assert!(true),
            Err(e) => assert!(false, "Failed to instantiate hash chain, error: {}", e),
        }
    }

}
