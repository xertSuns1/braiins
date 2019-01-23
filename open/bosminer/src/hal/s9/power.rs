// TODO remove thread specific code
use std;
use std::io;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use byteorder::{BigEndian, ByteOrder};

use embedded_hal::blocking::i2c::{Read, Write};
use linux_embedded_hal::I2cdev;

/// Voltage controller requires periodic heart beat messages to be sent
const VOLTAGE_CTRL_HEART_BEAT_PERIOD: Duration = Duration::from_millis(1000);

/// Default timeout required for I2C transactions to succeed
const I2C_TIMEOUT_MS: u64 = 500;

const PIC_BASE_ADDRESS: u8 = 0x50;

const PIC_COMMAND_1: u8 = 0x55;
const PIC_COMMAND_2: u8 = 0xAA;

// All commands provided by the PIC based voltage controller
const SET_PIC_FLASH_POINTER: u8 = 0x01;
const _SEND_DATA_TO_IIC: u8 = 0x02;
const READ_DATA_FROM_IIC: u8 = 0x03;
const _ERASE_IIC_FLASH: u8 = 0x04;
const _WRITE_DATA_INTO_PIC: u8 = 0x05;
const JUMP_FROM_LOADER_TO_APP: u8 = 0x06;
const RESET_PIC: u8 = 0x07;
const GET_PIC_FLASH_POINTER: u8 = 0x08;
const _ERASE_PIC_APP_PROGRAM: u8 = 0x09;
const SET_VOLTAGE: u8 = 0x10;
const _SET_VOLTAGE_TIME: u8 = 0x11;
const _SET_HASH_BOARD_ID: u8 = 0x12;
const _GET_HASH_BOARD_ID: u8 = 0x13;
const _SET_HOST_MAC_ADDRESS: u8 = 0x14;
const ENABLE_VOLTAGE: u8 = 0x15;
const SEND_HEART_BEAT: u8 = 0x16;
const GET_PIC_SOFTWARE_VERSION: u8 = 0x17;
const GET_VOLTAGE: u8 = 0x18;
const _GET_DATE: u8 = 0x19;
const _GET_WHICH_MAC: u8 = 0x20;
const _GET_MAC: u8 = 0x21;
const _WR_TEMP_OFFSET_VALUE: u8 = 0x22;
const RD_TEMP_OFFSET_VALUE: u8 = 0x23;

/// The PIC firmware in the voltage controller is expected to provide/return this version
pub const EXPECTED_VOLTAGE_CTRL_VERSION: u8 = 0x03;

/// Describes a voltage controller backend interface
pub trait VoltageCtrlBackend {
    /// Sends a Write transaction for a voltage controller on a particular hashboard
    /// * `data` - payload of the command
    fn write(&mut self, hashboard_idx: usize, command: u8, data: &[u8]) -> Result<(), io::Error>;
    /// Sends a Read transaction for a voltage controller on a particular hashboard
    /// * `length` - size of the expected response in bytes
    fn read(&mut self, hashboard_idx: usize, command: u8, length: u8)
        -> Result<Vec<u8>, io::Error>;

    /// Custom clone implementation
    /// TODO: review how this could be eliminated
    fn clone(&self) -> Self;
}

/// Newtype that represents an I2C voltage controller communication backend
/// S9 devices have a single I2C master that manages the voltage controllers on all hashboards.
/// Therefore, this will be a single communication instance
/// TODO: consider removing the type parameter as it will always be an I2cDev
pub struct VoltageCtrlI2cBlockingBackend<T> {
    inner: T,
}

impl<T> VoltageCtrlI2cBlockingBackend<T> {
    /// Calculates I2C address of the controller based on hashboard index.
    fn get_i2c_address(hashboard_idx: usize) -> u8 {
        PIC_BASE_ADDRESS + hashboard_idx as u8 - 1
    }
}

impl VoltageCtrlI2cBlockingBackend<I2cdev> {
    /// Instantiates a new I2C backend
    /// * `i2c_interface_num` - index of the I2C interface in Linux dev filesystem
    pub fn new(i2c_interface_num: usize) -> Self {
        Self {
            inner: I2cdev::new(format!("/dev/i2c-{}", i2c_interface_num)).unwrap(),
        }
    }
}

impl<T, E> VoltageCtrlBackend for VoltageCtrlI2cBlockingBackend<T>
where
    T: Read<Error = E> + Write<Error = E>,
    E: std::error::Error,
{
    fn write(&mut self, hashboard_idx: usize, command: u8, data: &[u8]) -> Result<(), io::Error> {
        let command_bytes = [&[PIC_COMMAND_1, PIC_COMMAND_2, command], data].concat();
        self.inner
            .write(Self::get_i2c_address(hashboard_idx), &command_bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("I2C write error: {}", e)))?;
        // I2C transactions require a delay, so that the Linux driver processes them properly
        // TODO: investigate this topic
        thread::sleep(Duration::from_millis(I2C_TIMEOUT_MS));
        Ok(())
    }

    fn read(
        &mut self,
        hashboard_idx: usize,
        command: u8,
        length: u8,
    ) -> Result<Vec<u8>, io::Error> {
        self.write(hashboard_idx, command, &[]).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("I2C read error in sending command: {}", e),
            )
        })?;
        let mut result = vec![0; length as usize];
        self.inner
            .read(Self::get_i2c_address(hashboard_idx), &mut result)
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("I2C read error in receiving data: {}", e),
                )
            })?;
        Ok(result)
    }

    fn clone(&self) -> Self {
        unimplemented!();
    }
}

pub struct VoltageCtrlI2cSharedBlockingBackend<T>(Arc<Mutex<T>>);

impl<T> VoltageCtrlI2cSharedBlockingBackend<T>
where
    T: VoltageCtrlBackend,
{
    pub fn new(backend: T) -> Self {
        VoltageCtrlI2cSharedBlockingBackend(Arc::new(Mutex::new(backend)))
    }
}

impl VoltageCtrlBackend
    for VoltageCtrlI2cSharedBlockingBackend<VoltageCtrlI2cBlockingBackend<I2cdev>>
{
    fn write(&mut self, hashboard_idx: usize, command: u8, data: &[u8]) -> Result<(), io::Error> {
        self.0.lock().unwrap().write(hashboard_idx, command, data)
    }

    fn read(
        &mut self,
        hashboard_idx: usize,
        command: u8,
        length: u8,
    ) -> Result<Vec<u8>, io::Error> {
        self.0.lock().unwrap().read(hashboard_idx, command, length)
    }

    /// Custom clone implementation that clones the atomic reference counting instance (Arc) only is
    /// needed so that we can share the backend instance. Unfortunately, we cannot implement the
    /// std::clone::Clone trait for now as it transitively puts additional requirements on the
    /// backend type parameter 'T'.
    /// TODO: review how this could be eliminated
    fn clone(&self) -> Self {
        VoltageCtrlI2cSharedBlockingBackend(self.0.clone())
    }
}

/// Represents a voltage controller for a particular hashboard
pub struct VoltageCtrl<T> {
    // Backend that carries out the operation
    backend: T,
    /// Identifies the hashboard
    hashboard_idx: usize,
}

/// Aliasing
type VoltageCtrlResult<T> = Result<T, io::Error>;

impl<T> VoltageCtrl<T>
where
    T: 'static + VoltageCtrlBackend + Send,
{
    fn read(&mut self, command: u8, length: u8) -> Result<Vec<u8>, io::Error> {
        self.backend.read(self.hashboard_idx, command, length)
    }

    fn write(&mut self, command: u8, data: &[u8]) -> Result<(), io::Error> {
        self.backend.write(self.hashboard_idx, command, data)
    }

    pub fn reset(&mut self) -> VoltageCtrlResult<()> {
        self.write(RESET_PIC, &[])
    }

    pub fn jump_from_loader_to_app(&mut self) -> VoltageCtrlResult<()> {
        self.write(JUMP_FROM_LOADER_TO_APP, &[])
    }

    pub fn get_version(&mut self) -> VoltageCtrlResult<u8> {
        Ok(self.read(GET_PIC_SOFTWARE_VERSION, 1)?[0])
    }

    pub fn set_flash_pointer(&mut self, address: u16) -> VoltageCtrlResult<()> {
        let mut address_bytes = [0; 2];
        BigEndian::write_u16(&mut address_bytes, address);
        self.write(SET_PIC_FLASH_POINTER, &[address_bytes[0], address_bytes[1]])
    }

    pub fn get_flash_pointer(&mut self) -> VoltageCtrlResult<u16> {
        let address_bytes = self.read(GET_PIC_FLASH_POINTER, 1)?;
        Ok(BigEndian::read_u16(&address_bytes))
    }

    pub fn read_data_from_iic(&mut self) -> VoltageCtrlResult<[u8; 16]> {
        let data = self.read(READ_DATA_FROM_IIC, 16)?;
        let mut data_array = [0; 16];
        data_array.copy_from_slice(&data);
        Ok(data_array)
    }

    pub fn enable_voltage(&mut self) -> VoltageCtrlResult<()> {
        self.write(ENABLE_VOLTAGE, &[true as u8])
    }

    pub fn disable_voltage(&mut self) -> VoltageCtrlResult<()> {
        self.write(ENABLE_VOLTAGE, &[false as u8])
    }

    pub fn set_voltage(&mut self, value: u8) -> VoltageCtrlResult<()> {
        self.write(SET_VOLTAGE, &[value])
    }

    pub fn get_voltage(&mut self) -> VoltageCtrlResult<u8> {
        Ok(self.read(GET_VOLTAGE, 1)?[0])
    }

    pub fn send_heart_beat(&mut self) -> VoltageCtrlResult<()> {
        self.write(SEND_HEART_BEAT, &[])
    }

    pub fn get_temperature_offset(&mut self) -> VoltageCtrlResult<u64> {
        let offset = self.read(RD_TEMP_OFFSET_VALUE, 8)?;
        Ok(BigEndian::read_u64(&offset))
    }

    /// Creates a new voltage controller
    pub fn new(backend: T, hashboard_idx: usize) -> Self {
        Self {
            backend,
            hashboard_idx,
        }
    }

    /// Helper method that sends heartbeat to the voltage controller at regular intervals
    ///
    /// The reason is to notify the voltage controller that we are alive so that it wouldn't
    /// cut-off power supply to the hashing chips on the board.
    /// TODO threading should be only part of some test profile
    pub fn start_heart_beat_task(&self) -> JoinHandle<()> {
        let hb_backend = self.backend.clone();
        let idx = self.hashboard_idx;
        let handle = thread::Builder::new()
            .name(format!("board[{}]: Voltage Ctrl heart beat", self.hashboard_idx).into())
            .spawn(move || {
                let mut voltage_ctrl = Self::new(hb_backend, idx);
                loop {
                    let now = SystemTime::now();
                    voltage_ctrl.send_heart_beat().unwrap();

                    println!("{:?}: Heartbeat for board {}", now, idx);
                    // evaluate how much time it took to send the heart beat and sleep for the rest
                    // of the heart beat period
                    let elapsed = now
                        .elapsed()
                        .map_err(|e| {
                            io::Error::new(
                                io::ErrorKind::Other,
                                format!("System time error: {}", e),
                            )
                        })
                        .unwrap();
                    // sleep only if we have not exceeded the heart beat period. This makes the
                    // code more robust when running it in debugger to prevent underflow time
                    // subtraction
                    if elapsed < VOLTAGE_CTRL_HEART_BEAT_PERIOD {
                        thread::sleep(VOLTAGE_CTRL_HEART_BEAT_PERIOD - elapsed);
                    }
                }
            })
            .unwrap();
        handle
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_address() {
        let addr = VoltageCtrlI2cBlockingBackend::<I2cdev>::get_i2c_address(8);
        let expected_addr = 0x57u8;
        assert_eq!(addr, expected_addr, "Unexpected hashboard I2C address");
    }
}
