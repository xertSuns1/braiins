use super::error::{self, ErrorKind};
use super::icarus;

use crate::hal;
use crate::work;

use failure::{Fail, ResultExt};

use std::convert::TryInto;
use std::mem::size_of;
use std::time::Duration;

const CP210X_TYPE_OUT: u8 = 0x41;
const CP210X_REQUEST_IFC_ENABLE: u8 = 0x00;
const CP210X_REQUEST_DATA: u8 = 0x07;
const CP210X_REQUEST_BAUD: u8 = 0x1e;

const CP210X_VALUE_UART_ENABLE: u16 = 0x0001;
const CP210X_VALUE_DATA: u16 = 0x0303;
const CP210X_DATA_BAUD: u32 = 115200;

pub struct BlockErupter<'a> {
    context: &'a libusb::Context,
    device: libusb::DeviceHandle<'a>,
}

impl<'a> BlockErupter<'a> {
    pub fn new(context: &'a libusb::Context, device: libusb::DeviceHandle<'a>) -> Self {
        Self { context, device }
    }

    /// Try to find Block Erupter connected to USB
    pub fn find(context: &'a libusb::Context) -> Option<Self> {
        context
            .open_device_with_vid_pid(icarus::ID_VENDOR, icarus::ID_PRODUCT)
            .map(|device| Self::new(context, device))
    }

    /// Initialize Block Erupter device to accept work to solution
    pub fn init(&mut self) -> error::Result<()> {
        self.device
            .reset()
            .with_context(|_| ErrorKind::Usb("cannot reset device"))?;

        if self.context.supports_detach_kernel_driver() {
            if self
                .device
                .kernel_driver_active(icarus::DEVICE_IFACE)
                .with_context(|_| ErrorKind::Usb("cannot detect kernel driver"))?
            {
                self.device
                    .detach_kernel_driver(icarus::DEVICE_IFACE)
                    .with_context(|_| ErrorKind::Usb("cannot detach kernel driver"))?;
            }
        }

        self.device
            .set_active_configuration(icarus::DEVICE_CONFIGURATION)
            .with_context(|_| ErrorKind::Usb("cannot set active configuration"))?;

        // enable the UART
        self.device
            .write_control(
                CP210X_TYPE_OUT,
                CP210X_REQUEST_IFC_ENABLE,
                CP210X_VALUE_UART_ENABLE,
                0,
                &[],
                icarus::WAIT_TIMEOUT,
            )
            .with_context(|_| ErrorKind::Usb("cannot enable UART"))?;
        // set data control
        self.device
            .write_control(
                CP210X_TYPE_OUT,
                CP210X_REQUEST_DATA,
                CP210X_VALUE_DATA,
                0,
                &[],
                icarus::WAIT_TIMEOUT,
            )
            .with_context(|_| ErrorKind::Usb("cannot set data control"))?;
        // set the baud
        self.device
            .write_control(
                CP210X_TYPE_OUT,
                CP210X_REQUEST_BAUD,
                0,
                0,
                &CP210X_DATA_BAUD.to_le_bytes(),
                icarus::WAIT_TIMEOUT,
            )
            .with_context(|_| ErrorKind::Usb("cannot set baud rate"))?;

        Ok(())
    }

    /// Send new work to the device
    /// All old work is interrupted immediately and the search space is restarted for the new work.  
    pub fn send_work(&self, work: icarus::WorkPayload) -> error::Result<()> {
        self.device
            .write_bulk(icarus::WRITE_ADDR, &work.into_bytes(), icarus::WAIT_TIMEOUT)
            .with_context(|_| ErrorKind::Usb("cannot send work"))?;

        Ok(())
    }

    /// Wait for specified amount of time to find the nonce for current work
    /// The work have to be previously send using `send_work` method.
    /// More solution may exist so this method must be called multiple times to get all of them.
    /// When all search space is exhausted then the chip stops finding new nonce. The maximal time
    /// of searching is constant for this chip and after this time no new solution is found.
    /// The `None` is returned then timeout occurs and any nonce is found.
    /// It is possible that during sending new work the nonce for old one can be found and returned
    /// from this method!
    pub fn wait_for_nonce(&self, timeout: Duration) -> error::Result<Option<u32>> {
        let mut nonce = [0u8; size_of::<u32>()];
        match self
            .device
            .read_bulk(icarus::READ_ADDR, &mut nonce, timeout)
        {
            Ok(n) => {
                if n != size_of::<u32>() {
                    Err(ErrorKind::Usb("read incorrect number of bytes"))?
                };
                Ok(u32::from_le_bytes(nonce)
                    .try_into()
                    .expect("slice with incorrect length"))
            }
            Err(libusb::Error::Timeout) => Ok(None),
            Err(e) => Err(e.context(ErrorKind::Usb("cannot read nonce")).into()),
        }
    }

    /// Converts Block Erupter device into iterator which solving generated work
    pub fn into_solver(self, work_generator: work::Generator) -> BlockErupterSolver<'a> {
        BlockErupterSolver::new(self, work_generator)
    }
}

/// Wrap the Block Erupter device and work generator to implement iterable object which solves
/// incoming work and tries to find solution which is returned as an unique mining work solution
pub struct BlockErupterSolver<'a> {
    _device: BlockErupter<'a>,
    _work_generator: work::Generator,
    stop_reason: error::Result<()>,
}

impl<'a> BlockErupterSolver<'a> {
    fn new(_device: BlockErupter<'a>, _work_generator: work::Generator) -> Self {
        Self {
            _device,
            _work_generator,
            stop_reason: Ok(()),
        }
    }

    /// Consume the iterator and return the reason of stream termination
    pub fn get_stop_reason(self) -> error::Result<()> {
        self.stop_reason
    }
}

impl<'a> Iterator for BlockErupterSolver<'a> {
    type Item = hal::UniqueMiningWorkSolution;

    /// Waits for new work and send it to the Block Erupter device
    /// When the solution is found then the result is returned as an unique mining work solution.
    /// When an error occurs then `None` is returned and the failure reason can be obtained with
    /// `get_stop_reason` method which consumes the iterator.
    fn next(&mut self) -> Option<hal::UniqueMiningWorkSolution> {
        None
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::hal::BitcoinJob;
    use crate::test_utils;

    use std::ops::{Deref, DerefMut};
    use std::sync;
    use std::time::SystemTime;

    use lazy_static::lazy_static;

    lazy_static! {
        pub static ref USB_CONTEXT_MUTEX: sync::Mutex<()> = sync::Mutex::new(());
        pub static ref USB_CONTEXT: libusb::Context =
            libusb::Context::new().expect("cannot create new USB context");
    }

    struct BlockErupterGuard<'a> {
        device: BlockErupter<'a>,
        // context guard have to be dropped after block erupter device
        // do not change the order of members!
        _context_guard: sync::MutexGuard<'a, ()>,
    }

    impl<'a> BlockErupterGuard<'a> {
        fn new(device: BlockErupter<'a>, _context_guard: sync::MutexGuard<'a, ()>) -> Self {
            Self {
                device,
                _context_guard,
            }
        }
    }

    impl<'a> Deref for BlockErupterGuard<'a> {
        type Target = BlockErupter<'a>;

        fn deref(&self) -> &Self::Target {
            &self.device
        }
    }

    impl<'a> DerefMut for BlockErupterGuard<'a> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.device
        }
    }

    /// Synchronization function to get only one device at one moment to allow parallel tests
    fn get_block_erupter<'a>() -> BlockErupterGuard<'a> {
        // lock USB context for mutual exclusion
        let context_guard = USB_CONTEXT_MUTEX.lock().expect("cannot lock USB context");

        let mut device =
            BlockErupter::find(&*USB_CONTEXT).expect("cannot find Block Erupter device");
        // try to initialize Block Erupter
        device.init().expect("Block Erupter initialization failed");

        // the USB context will be unlocked at the end of a test using this device
        BlockErupterGuard::new(device, context_guard)
    }

    #[test]
    fn test_block_erupter_init() {
        let _device = get_block_erupter();
    }

    #[test]
    fn test_block_erupter_io() {
        let device = get_block_erupter();

        for (i, block) in test_utils::TEST_BLOCKS.iter().enumerate() {
            let work = icarus::WorkPayload::new(
                &block.midstate,
                block.merkle_root_tail(),
                block.time(),
                block.bits(),
            );

            // send new work generated from test block
            device
                .send_work(work)
                .expect("cannot send work to Block Erupter");

            // wait for solution
            let timeout = icarus::MAX_READ_TIME;
            let mut timeout_rem = timeout;
            let mut nonce_found = false;

            let start = SystemTime::now();
            loop {
                match device
                    .wait_for_nonce(timeout_rem)
                    .expect("cannot read nonce from Block Erupter")
                {
                    None => break,
                    Some(nonce) => {
                        if block.nonce == nonce {
                            nonce_found = true;
                            break;
                        }
                    }
                }
                let duration = SystemTime::now()
                    .duration_since(start)
                    .expect("SystemTime::duration_since failed");
                timeout_rem = timeout
                    .checked_sub(duration)
                    .unwrap_or(Duration::from_millis(1));
            }

            assert!(nonce_found, "solution for block {} cannot be found", i);
        }
    }
}
