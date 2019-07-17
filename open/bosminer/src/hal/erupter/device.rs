use super::error::{self, ErrorKind};
use super::icarus;

use failure::ResultExt;

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
}

#[cfg(test)]
pub mod test {
    use super::*;

    #[test]
    fn test_block_erupter_init() {
        // get lib USB context
        let context = libusb::Context::new().expect("cannot create new USB context");
        let mut device = BlockErupter::find(&context).expect("cannot find Block Erupter device");
        // try to initialize Block Erupter
        device.init().expect("Block Erupter initialization failed");
    }
}
