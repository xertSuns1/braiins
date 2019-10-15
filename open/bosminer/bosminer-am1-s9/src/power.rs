// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

use ii_logging::macros::*;

// TODO remove thread specific code
use std::convert::TryInto;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use super::error::{self, ErrorKind};
use failure::ResultExt;

use embedded_hal::blocking::i2c::{Read, Write};
use linux_embedded_hal::I2cdev;

use ii_async_compat::sleep;

/// Voltage controller requires periodic heart beat messages to be sent
const VOLTAGE_CTRL_HEART_BEAT_PERIOD: Duration = Duration::from_millis(1000);

const PIC_BASE_ADDRESS: u8 = 0x50;

const PIC_COMMAND_1: u8 = 0x55;
const PIC_COMMAND_2: u8 = 0xAA;

// All commands provided by the PIC based voltage controller
const SET_PIC_FLASH_POINTER: u8 = 0x01;
#[allow(dead_code)]
const SEND_DATA_TO_IIC: u8 = 0x02;
const READ_DATA_FROM_IIC: u8 = 0x03;
#[allow(dead_code)]
const ERASE_IIC_FLASH: u8 = 0x04;
#[allow(dead_code)]
const WRITE_DATA_INTO_PIC: u8 = 0x05;
const JUMP_FROM_LOADER_TO_APP: u8 = 0x06;
const RESET_PIC: u8 = 0x07;
const GET_PIC_FLASH_POINTER: u8 = 0x08;
#[allow(dead_code)]
const ERASE_PIC_APP_PROGRAM: u8 = 0x09;
const SET_VOLTAGE: u8 = 0x10;
#[allow(dead_code)]
const SET_VOLTAGE_TIME: u8 = 0x11;
#[allow(dead_code)]
const SET_HASH_BOARD_ID: u8 = 0x12;
#[allow(dead_code)]
const GET_HASH_BOARD_ID: u8 = 0x13;
#[allow(dead_code)]
const SET_HOST_MAC_ADDRESS: u8 = 0x14;
const ENABLE_VOLTAGE: u8 = 0x15;
const SEND_HEART_BEAT: u8 = 0x16;
const GET_PIC_SOFTWARE_VERSION: u8 = 0x17;
const GET_VOLTAGE: u8 = 0x18;
#[allow(dead_code)]
const GET_DATE: u8 = 0x19;
#[allow(dead_code)]
const GET_WHICH_MAC: u8 = 0x20;
#[allow(dead_code)]
const GET_MAC: u8 = 0x21;
#[allow(dead_code)]
const WR_TEMP_OFFSET_VALUE: u8 = 0x22;
const RD_TEMP_OFFSET_VALUE: u8 = 0x23;

/// The PIC firmware in the voltage controller is expected to provide/return this version
pub const EXPECTED_VOLTAGE_CTRL_VERSION: u8 = 0x03;

/// Bundle voltage value with methods to convert it to/from various representations
#[derive(Clone, Copy)]
pub struct Voltage(f32);

impl Voltage {
    pub const fn from_volts(voltage: f32) -> Self {
        Self(voltage)
    }

    #[inline]
    pub fn as_volts(&self) -> f32 {
        self.0
    }

    /// These PIC conversion functions and coefficients are taken from
    /// bmminer source: getPICvoltageFromValue, getVolValueFromPICvoltage
    const VOLT_CONV_COEF_1: f32 = 1608.420446;
    const VOLT_CONV_COEF_2: f32 = 170.423497;

    pub fn as_pic_value(&self) -> error::Result<u8> {
        let pic_val = (Self::VOLT_CONV_COEF_1 - Self::VOLT_CONV_COEF_2 * self.0).round();
        if pic_val >= 0.0 && pic_val <= 255.0 {
            Ok(pic_val as u8)
        } else {
            Err(ErrorKind::Power("requested voltage out of range".to_string()).into())
        }
    }

    pub fn from_pic_value(pic_val: u8) -> Self {
        Self((Self::VOLT_CONV_COEF_1 - pic_val as f32) / Self::VOLT_CONV_COEF_2)
    }
}

/// Describes a voltage controller backend interface
pub trait Backend
where
    Self: Send + Sync,
{
    /// Sends a Write transaction for a voltage controller on a particular hashboard
    /// * `data` - payload of the command
    fn write(&mut self, hashboard_idx: usize, command: u8, data: &[u8]) -> error::Result<()>;
    /// Sends a Read transaction for a voltage controller on a particular hashboard
    /// * `length` - size of the expected response in bytes
    fn read(&mut self, hashboard_idx: usize, command: u8, length: u8) -> error::Result<Vec<u8>>;
}

/// Newtype that represents an I2C voltage controller communication backend
/// S9 devices have a single I2C master that manages the voltage controllers on all hashboards.
/// Therefore, this will be a single communication instance
pub struct I2cBackend {
    inner: I2cdev,
}

impl I2cBackend {
    /// Calculates I2C address of the controller based on hashboard index.
    fn get_i2c_address(hashboard_idx: usize) -> u8 {
        PIC_BASE_ADDRESS + hashboard_idx as u8 - 1
    }

    /// Instantiates a new I2C backend
    /// * `i2c_interface_num` - index of the I2C interface in Linux dev filesystem
    pub fn new(i2c_interface_num: usize) -> Self {
        Self {
            inner: I2cdev::new(format!("/dev/i2c-{}", i2c_interface_num))
                .expect("i2c instantiation failed"),
        }
    }
}

impl Backend for I2cBackend {
    fn write(&mut self, hashboard_idx: usize, command: u8, data: &[u8]) -> error::Result<()> {
        let command_bytes = [&[PIC_COMMAND_1, PIC_COMMAND_2, command], data].concat();
        self.inner
            .write(Self::get_i2c_address(hashboard_idx), &command_bytes)
            .with_context(|e| ErrorKind::I2c(e.to_string()))?;
        Ok(())
    }

    fn read(&mut self, hashboard_idx: usize, command: u8, length: u8) -> error::Result<Vec<u8>> {
        self.write(hashboard_idx, command, &[])
            .with_context(|e| ErrorKind::I2c(e.to_string()))?;
        let mut result = vec![0; length as usize];
        self.inner
            .read(Self::get_i2c_address(hashboard_idx), &mut result)
            .with_context(|e| ErrorKind::I2c(e.to_string()))?;
        Ok(result)
    }
}

pub struct SharedBackend<T>(Arc<Mutex<T>>);

impl<T> SharedBackend<T>
where
    T: Backend,
{
    pub fn new(backend: T) -> Self {
        SharedBackend(Arc::new(Mutex::new(backend)))
    }
}

impl<T> Backend for SharedBackend<T>
where
    T: Backend,
{
    fn write(&mut self, hashboard_idx: usize, command: u8, data: &[u8]) -> error::Result<()> {
        self.0
            .lock()
            .expect("locking failed")
            .write(hashboard_idx, command, data)
    }

    fn read(&mut self, hashboard_idx: usize, command: u8, length: u8) -> error::Result<Vec<u8>> {
        self.0
            .lock()
            .expect("locking failed")
            .read(hashboard_idx, command, length)
    }
}

impl<T> Clone for SharedBackend<T> {
    fn clone(&self) -> Self {
        SharedBackend(self.0.clone())
    }
}

/// Represents a voltage controller for a particular hashboard
///
/// NOTE: Some I2C PIC commands require explicit wait time before issuing new
/// commands.
///
/// For example, `reset` command requires to wait approx 500ms, because while
/// the PIC is booting up, it doesn't respond to I2C ACK when addressed.
/// This condition (NAK) manifests itself as Linux I2C driver returning error
/// (EIO) from write syscall.
///
/// Most of commands implemented bellow have correct timeout included,
/// but if you implement some new commands be sure to include timeout where
/// necessary (`SET_HOST_MAC_ADDRESS` requires one etc., check bmminer
/// sources).
#[derive(Clone)]
pub struct Control<T> {
    // Backend that carries out the operation
    backend: T,
    /// Identifies the hashboard
    hashboard_idx: usize,
}

impl<T> Control<T>
where
    T: 'static + Backend + Send + Clone,
{
    /// How long does it take to reset the PIC controller.
    const RESET_DELAY: Duration = Duration::from_millis(500);

    /// This constant is from `bmminer` sources and it works.
    /// I have no deeper insight on how was this constant determined.
    const BMMINER_DELAY: Duration = Duration::from_millis(100);

    fn read(&mut self, command: u8, length: u8) -> error::Result<Vec<u8>> {
        self.backend.read(self.hashboard_idx, command, length)
    }

    fn write(&mut self, command: u8, data: &[u8]) -> error::Result<()> {
        self.backend.write(self.hashboard_idx, command, data)
    }

    pub async fn reset(&mut self) -> error::Result<()> {
        self.write(RESET_PIC, &[])?;
        await!(sleep(Self::RESET_DELAY));
        Ok(())
    }

    pub async fn jump_from_loader_to_app(&mut self) -> error::Result<()> {
        self.write(JUMP_FROM_LOADER_TO_APP, &[])?;
        await!(sleep(Self::BMMINER_DELAY));
        Ok(())
    }

    pub fn get_version(&mut self) -> error::Result<u8> {
        Ok(self.read(GET_PIC_SOFTWARE_VERSION, 1)?[0])
    }

    pub fn set_flash_pointer(&mut self, address: u16) -> error::Result<()> {
        self.write(SET_PIC_FLASH_POINTER, &u16::to_be_bytes(address))
    }

    pub fn get_flash_pointer(&mut self) -> error::Result<u16> {
        let address_bytes = self.read(GET_PIC_FLASH_POINTER, 2)?;
        Ok(u16::from_be_bytes(
            address_bytes
                .as_slice()
                .try_into()
                .expect("incorrect slice length"),
        ))
    }

    pub fn read_data_from_iic(&mut self) -> error::Result<[u8; 16]> {
        let data = self.read(READ_DATA_FROM_IIC, 16)?;
        let mut data_array = [0; 16];
        data_array.copy_from_slice(&data);
        Ok(data_array)
    }

    pub fn enable_voltage(&mut self) -> error::Result<()> {
        self.write(ENABLE_VOLTAGE, &[true as u8])
    }

    pub fn disable_voltage(&mut self) -> error::Result<()> {
        self.write(ENABLE_VOLTAGE, &[false as u8])
    }

    pub async fn set_voltage(&mut self, voltage: Voltage) -> error::Result<()> {
        info!("Setting voltage to {}", voltage.as_volts());
        self.write(SET_VOLTAGE, &[voltage.as_pic_value()?])?;
        await!(sleep(Self::BMMINER_DELAY));
        Ok(())
    }

    pub fn get_voltage(&mut self) -> error::Result<u8> {
        Ok(self.read(GET_VOLTAGE, 1)?[0])
    }

    pub fn send_heart_beat(&mut self) -> error::Result<()> {
        self.write(SEND_HEART_BEAT, &[])
    }

    pub fn get_temperature_offset(&mut self) -> error::Result<u64> {
        let offset = self.read(RD_TEMP_OFFSET_VALUE, 8)?;
        Ok(u64::from_be_bytes(
            offset
                .as_slice()
                .try_into()
                .expect("incorrect slice length"),
        ))
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
                    voltage_ctrl
                        .send_heart_beat()
                        .expect("send_heart_beat failed");

                    //trace!("Heartbeat for board {}", idx);
                    // evaluate how much time it took to send the heart beat and sleep for the rest
                    // of the heart beat period
                    let elapsed = now
                        .elapsed()
                        .context("cannot measure elapsed time")
                        .unwrap();
                    // sleep only if we have not exceeded the heart beat period. This makes the
                    // code more robust when running it in debugger to prevent underflow time
                    // subtraction
                    if elapsed < VOLTAGE_CTRL_HEART_BEAT_PERIOD {
                        thread::sleep(VOLTAGE_CTRL_HEART_BEAT_PERIOD - elapsed);
                    }
                }
            })
            .expect("thread spawning failed");
        handle
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_address() {
        let addr = I2cBackend::get_i2c_address(8);
        let expected_addr = 0x57u8;
        assert_eq!(addr, expected_addr, "Unexpected hashboard I2C address");
    }

    #[test]
    fn test_voltage_to_pic() {
        assert_eq!(Voltage::from_volts(9.4).as_pic_value().unwrap(), 6);
        assert_eq!(Voltage::from_volts(8.9).as_pic_value().unwrap(), 92);
        assert_eq!(Voltage::from_volts(8.1).as_pic_value().unwrap(), 228);
        assert!(Voltage::from_volts(10.0).as_pic_value().is_err());
    }

    #[test]
    fn test_pic_to_voltage() {
        let epsilon = 0.01f32;
        let difference = Voltage::from_pic_value(6).as_volts() - 9.40;
        assert!(difference.abs() <= epsilon);
        let difference = Voltage::from_pic_value(92).as_volts() - 8.9;
        assert!(difference.abs() <= epsilon);
    }

    #[test]
    fn test_pic_boundary() {
        // pic=255
        assert!(Voltage::from_volts(7.941513170569432)
            .as_pic_value()
            .is_ok());
        // pic=256
        assert!(Voltage::from_volts(7.935645435089271)
            .as_pic_value()
            .is_err());
        // pic=0
        assert!(Voltage::from_volts(9.437785718010469)
            .as_pic_value()
            .is_ok());
        // pic=-1
        assert!(Voltage::from_volts(9.443653453490631)
            .as_pic_value()
            .is_err());
    }
}
