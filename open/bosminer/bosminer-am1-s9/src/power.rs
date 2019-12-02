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

pub mod firmware;

use ii_logging::macros::*;

// TODO remove thread specific code
use std::convert::TryInto;
use std::sync::Arc;
use std::time::Duration;

use crate::async_i2c::AsyncI2cDev;
use crate::error::{self, ErrorKind};
use crate::halt;

use futures::lock::Mutex;
use ii_async_compat::futures;
use ii_async_compat::tokio;
use tokio::time::delay_for;

/// Default initial voltage
pub const OPEN_CORE_VOLTAGE: Voltage = Voltage::from_volts(9.4);

/// Voltage controller requires periodic heart beat messages to be sent
const VOLTAGE_CTRL_HEART_BEAT_PERIOD: Duration = Duration::from_millis(1000);

const PIC_BASE_ADDRESS: u8 = 0x50;

const PIC_COMMAND_1: u8 = 0x55;
const PIC_COMMAND_2: u8 = 0xAA;

// All commands provided by the PIC based voltage controller
const SET_PIC_FLASH_POINTER: u8 = 0x01;
const SEND_DATA_TO_IIC: u8 = 0x02;
const READ_DATA_FROM_IIC: u8 = 0x03;
const ERASE_IIC_FLASH: u8 = 0x04;
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

/// Path to voltage controller PIC program
pub const PIC_PROGRAM_PATH: &'static str = "/etc/hash_s8_app.txt";

/// Bundle voltage value with methods to convert it to/from various representations
#[derive(Clone, Copy, PartialEq)]
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

/// Type that represents an I2C voltage controller communication backend
/// S9 devices have a single I2C master that manages the voltage controllers on all hashboards.
/// Therefore, this will be a single communication instance.
pub struct I2cBackend {
    inner: AsyncI2cDev,
}

impl I2cBackend {
    /// Number of times to retries if I2C transaction fails
    const I2C_NUM_RETRIES: usize = 15;
    /// Duration between successive tries
    const I2C_RETRY_DELAY: Duration = Duration::from_millis(100);

    /// Calculates I2C address of the controller based on hashboard index.
    fn get_i2c_address(hashboard_idx: usize) -> u8 {
        PIC_BASE_ADDRESS + hashboard_idx as u8 - 1
    }

    /// Instantiates a new I2C backend
    /// * `i2c_interface_num` - index of the I2C interface in Linux dev filesystem
    pub fn new(i2c_interface_num: usize) -> Self {
        Self {
            inner: AsyncI2cDev::open(format!("/dev/i2c-{}", i2c_interface_num))
                .expect("I2C instantiation failed"),
        }
    }

    /// Attempt to write a byte to power controller on I2C.
    /// If write fails then retry (at most `I2C_NUM_RETRIES`).
    async fn write_retry(&self, hashboard_idx: usize, data: u8) -> error::Result<()> {
        let mut tries_left: usize = Self::I2C_NUM_RETRIES;
        loop {
            let ret = self
                .inner
                .write(Self::get_i2c_address(hashboard_idx), vec![data])
                .await;
            if ret.is_ok() {
                return ret;
            }
            tries_left -= 1;
            if tries_left == 0 {
                return ret;
            }
            warn!(
                "I2C transaction on hashboard {} failed, retrying...",
                hashboard_idx
            );
            delay_for(Self::I2C_RETRY_DELAY).await;
        }
    }

    /// Perform a write command to power controller on I2C
    pub async fn write(&self, hashboard_idx: usize, command: u8, data: &[u8]) -> error::Result<()> {
        let command_bytes = [&[PIC_COMMAND_1, PIC_COMMAND_2, command], data].concat();
        for byte in command_bytes.into_iter() {
            self.write_retry(hashboard_idx, byte).await?;
        }
        Ok(())
    }

    /// Perform a read command from power controller on I2C
    pub async fn read(
        &self,
        hashboard_idx: usize,
        command: u8,
        length: usize,
    ) -> error::Result<Vec<u8>> {
        self.write(hashboard_idx, command, &[]).await?;
        // Read has to be done via single-byte I2C transactions.
        // If multiple bytes are read within single transaction, only first byte is valid. The
        // rest is garbage.
        let mut reply = Vec::with_capacity(length);
        for _ in 0..length {
            let byte = self
                .inner
                .read(Self::get_i2c_address(hashboard_idx), length)
                .await?;
            reply.push(byte[0]);
        }
        Ok(reply)
    }
}

/// This is per-hashboard voltage controller backend (knows its hashboard_idx).
/// All hashboards share one I2C master bus, so we use this structure to manage concurrent access
/// to hashboard power controller (ie. if we want to delay operations on one voltage
/// controller, we lock this structure instead of locking the whole I2C bus).
struct HashboardBackend {
    // I2cBackend is shared amongst all hashchains
    backend: Arc<I2cBackend>,
    hashboard_idx: usize,
}

impl HashboardBackend {
    fn new(backend: Arc<I2cBackend>, hashboard_idx: usize) -> Self {
        Self {
            backend,
            hashboard_idx,
        }
    }

    async fn write(&self, command: u8, data: &[u8]) -> error::Result<()> {
        self.backend.write(self.hashboard_idx, command, data).await
    }

    async fn read(&self, command: u8, length: usize) -> error::Result<Vec<u8>> {
        self.backend.read(self.hashboard_idx, command, length).await
    }
}

/// Type to represent number of PIC flash words
/// TODO: implement arithmetic on `PicWords`, `PicAddress`, add constructors, bounds, etc.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PicWords(pub u16);

impl PicWords {
    pub fn to_bytes(&self) -> usize {
        self.0 as usize * 2
    }

    pub fn from_bytes(num_bytes: usize) -> Self {
        assert_eq!(num_bytes % 2, 0);
        assert!(num_bytes / 2 <= std::u16::MAX as usize);
        Self((num_bytes / 2) as u16)
    }
}

/// PIC address (one word is 14 bits, which is represented here by u16)
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PicAddress(pub u16);

impl PicAddress {
    pub fn distance_to(&self, end: PicAddress) -> PicWords {
        assert!(self.0 <= end.0);
        PicWords(end.0 - self.0 + 1)
    }

    pub fn offset(&self, distance: PicWords) -> PicAddress {
        PicAddress(self.0 + distance.0)
    }
}

/// Utility function to calculate number of whole blocks and remainder
fn blocks(size: usize, block_size: usize) -> (usize, usize) {
    (size / block_size, size % block_size)
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
pub struct Control {
    /// Backend that carries out the operation
    backend: Mutex<HashboardBackend>,
    /// Tracks current voltage
    /// Locks: first take this, then `backend`
    current_voltage: Mutex<Option<Voltage>>,
}

impl Control {
    /// How long does it take to reset the PIC controller.
    const RESET_DELAY: Duration = Duration::from_millis(500);

    /// This constant is from `bmminer` sources and it works.
    /// I have no deeper insight on how was this constant determined.
    const BMMINER_DELAY: Duration = Duration::from_millis(100);

    /// Flash sector size
    pub const FLASH_SECTOR_WORDS: usize = 32;

    /// Number of bytes in `SEND_DATA_TO_IIC` and `READ_DATA_FROM_IIC` command response/reply
    pub const FLASH_XFER_BLOCK_SIZE_BYTES: usize = 16;

    async fn read(&self, command: u8, length: usize) -> error::Result<Vec<u8>> {
        self.backend.lock().await.read(command, length).await
    }

    async fn write(&self, command: u8, data: &[u8]) -> error::Result<()> {
        self.backend.lock().await.write(command, data).await
    }

    /// Do a write followed by a delay with locks held to let voltage controller finish
    /// the operation.
    async fn write_delay(&self, command: u8, data: &[u8], delay: Duration) -> error::Result<()> {
        let backend = self.backend.lock().await;
        backend.write(command, data).await?;
        // wait for delay while holding lock
        delay_for(delay).await;
        Ok(())
    }

    pub async fn send_data_to_iic(&self, data: &[u8]) -> error::Result<()> {
        self.write(SEND_DATA_TO_IIC, data).await
    }

    /// Erase one flash sector (64 bytes)
    pub async fn erase_flash_sector(&self) -> error::Result<()> {
        self.write_delay(ERASE_IIC_FLASH, &[], Self::BMMINER_DELAY)
            .await
    }

    /// Erase `num_words` (must be divisible by `FLASH_SECTOR_WORDS`) starting from address
    /// `start` (this must be also divisible by `FLASH_SECTOR_WORDS`, because the erase is
    /// "page erase")
    pub async fn erase_flash(&self, start: PicAddress, num_words: PicWords) -> error::Result<()> {
        assert_eq!(start.0 as usize % Self::FLASH_SECTOR_WORDS, 0);
        let (num_blocks, odd_words) = blocks(num_words.0 as usize, Self::FLASH_SECTOR_WORDS);
        assert_eq!(odd_words, 0);
        self.set_flash_pointer_check(start).await?;
        for _ in 0..num_blocks {
            self.erase_flash_sector().await?;
        }
        Ok(())
    }

    /// Read `num_words` of PIC words from address `start`. The number of bytes transfered must
    /// be divisible `FLASH_XFER_BLOCK_SIZE_BYTES`.
    /// Beware that you are specifying the size in `PicWords`, and you get twice as many bytes
    /// as a result.
    pub async fn read_flash(
        &self,
        start: PicAddress,
        num_words: PicWords,
    ) -> error::Result<Vec<u8>> {
        let (num_blocks, odd_bytes) =
            blocks(num_words.to_bytes(), Self::FLASH_XFER_BLOCK_SIZE_BYTES);
        assert_eq!(odd_bytes, 0);
        self.set_flash_pointer_check(start).await?;
        let mut data = Vec::new();
        for _ in 0..num_blocks {
            data.push(self.read_data_from_iic().await?);
        }
        Ok(data.concat())
    }

    /// Write `data` to flash starting at address `start`. The numver of bytes written must be divisible
    /// by `Self::FLASH_XFER_BLOCK_SIZE_BYTES`.
    pub async fn write_flash(&self, start: PicAddress, data: &[u8]) -> error::Result<()> {
        let (_, odd_bytes) = blocks(data.len(), Self::FLASH_XFER_BLOCK_SIZE_BYTES);
        assert_eq!(odd_bytes, 0);
        self.set_flash_pointer_check(start).await?;
        for chunk in data.chunks(Self::FLASH_XFER_BLOCK_SIZE_BYTES) {
            self.send_data_to_iic(chunk).await?;
            self.write_data_to_flash().await?;
        }
        Ok(())
    }

    pub async fn reset(&self) -> error::Result<()> {
        info!("Voltage controller reset");
        self.write_delay(RESET_PIC, &[], Self::RESET_DELAY).await
    }

    pub async fn jump_from_loader_to_app(&self) -> error::Result<()> {
        self.write_delay(JUMP_FROM_LOADER_TO_APP, &[], Self::BMMINER_DELAY)
            .await?;
        info!("Voltage controller application started");
        Ok(())
    }

    pub async fn get_version(&self) -> error::Result<u8> {
        let version = self.read(GET_PIC_SOFTWARE_VERSION, 1).await?[0];
        info!("Voltage controller firmware version {:#04x}", version);
        Ok(version)
    }

    pub async fn write_data_to_flash(&self) -> error::Result<()> {
        self.write_delay(WRITE_DATA_INTO_PIC, &[], Self::BMMINER_DELAY)
            .await
    }

    pub async fn set_flash_pointer(&self, address: PicAddress) -> error::Result<()> {
        self.write(SET_PIC_FLASH_POINTER, &u16::to_be_bytes(address.0))
            .await
    }

    pub async fn get_flash_pointer(&self) -> error::Result<PicAddress> {
        let address_bytes = self.read(GET_PIC_FLASH_POINTER, 2).await?;
        Ok(PicAddress(u16::from_be_bytes(
            address_bytes
                .as_slice()
                .try_into()
                .expect("incorrect slice length"),
        )))
    }

    /// "Safe" variant of `set_flash_pointer` that checks that the pointer has really been set
    /// at the right place
    pub async fn set_flash_pointer_check(&self, want_address: PicAddress) -> error::Result<()> {
        self.set_flash_pointer(want_address).await?;
        let current_address = self.get_flash_pointer().await?;
        if current_address != want_address {
            Err(ErrorKind::Power(format!(
                "PIC should be at address {:#x?} but it's at address {:#x?}",
                want_address, current_address
            )))?
        }
        Ok(())
    }

    pub async fn read_data_from_iic(
        &self,
    ) -> error::Result<[u8; Self::FLASH_XFER_BLOCK_SIZE_BYTES]> {
        let data = self
            .read(READ_DATA_FROM_IIC, Self::FLASH_XFER_BLOCK_SIZE_BYTES)
            .await?;
        let mut data_array = [0; Self::FLASH_XFER_BLOCK_SIZE_BYTES];
        data_array.copy_from_slice(&data);
        Ok(data_array)
    }

    pub async fn enable_voltage(&self) -> error::Result<()> {
        self.write(ENABLE_VOLTAGE, &[true as u8]).await
    }

    pub async fn disable_voltage(&self) -> error::Result<()> {
        self.write(ENABLE_VOLTAGE, &[false as u8]).await
    }

    pub async fn set_voltage(&self, voltage: Voltage) -> error::Result<()> {
        let mut current_voltage = self.current_voltage.lock().await;
        if *current_voltage != Some(voltage) {
            info!(
                "Setting voltage to {} (was: {:?})",
                voltage.as_volts(),
                current_voltage.map(|v| v.as_volts())
            );
            self.write_delay(SET_VOLTAGE, &[voltage.as_pic_value()?], Self::BMMINER_DELAY)
                .await?;
            *current_voltage = Some(voltage);
        }
        Ok(())
    }

    pub async fn get_voltage(&self) -> error::Result<u8> {
        Ok(self.read(GET_VOLTAGE, 1).await?[0])
    }

    pub async fn send_heart_beat(&self) -> error::Result<()> {
        self.write(SEND_HEART_BEAT, &[]).await
    }

    pub async fn get_temperature_offset(&self) -> error::Result<u64> {
        let offset = self.read(RD_TEMP_OFFSET_VALUE, 8).await?;
        Ok(u64::from_be_bytes(
            offset
                .as_slice()
                .try_into()
                .expect("incorrect slice length"),
        ))
    }

    /// Load PIC program onto the voltage controller
    pub async fn program_pic(&self, program: &firmware::PicProgram) -> error::Result<()> {
        if program.bytes.len() % Self::FLASH_XFER_BLOCK_SIZE_BYTES != 0 {
            // This is irrelevant now (we check size), but otherwise it's required because
            // the self-programmer can only load whole blocks
            Err(ErrorKind::Power(format!(
                "PIC program size not divisible by {}",
                Self::FLASH_XFER_BLOCK_SIZE_BYTES
            )))?
        }
        self.reset().await?;
        self.erase_flash(program.load_addr, program.prog_size)
            .await?;
        self.write_flash(program.load_addr, &program.bytes[..])
            .await?;
        if self.get_flash_pointer().await? != program.load_addr.offset(program.prog_size) {
            Err(ErrorKind::Power(
                "flash pointer ended at invalid address".into(),
            ))?
        }
        Ok(())
    }

    /// Creates a new voltage controller
    pub fn new(backend: Arc<I2cBackend>, hashboard_idx: usize) -> Self {
        Self {
            backend: Mutex::new(HashboardBackend::new(backend, hashboard_idx)),
            current_voltage: Mutex::new(None),
        }
    }

    async fn reset_and_start_app(&self) -> error::Result<u8> {
        self.reset().await?;
        self.jump_from_loader_to_app().await?;
        Ok(self.get_version().await?)
    }

    /// Initialize voltage controller
    /// TODO: decouple this code from `halt_receiver`
    pub async fn init(self: Arc<Self>, halt_receiver: halt::Receiver) -> error::Result<()> {
        let version = self.reset_and_start_app().await?;
        // TODO accept multiple
        if version != EXPECTED_VOLTAGE_CTRL_VERSION {
            info!("Bad firmware version! Reloading firmware...");
            let program = firmware::PicProgram::read(PIC_PROGRAM_PATH)?;
            self.program_pic(&program).await?;

            let version = self.reset_and_start_app().await?;
            if version != EXPECTED_VOLTAGE_CTRL_VERSION {
                info!("Firmware reloading failed, still bad firmware version...");
                Err(ErrorKind::UnexpectedVersion(
                    "voltage controller firmware".to_string(),
                    version.to_string(),
                    EXPECTED_VOLTAGE_CTRL_VERSION.to_string(),
                ))?
            }
        }
        self.set_voltage(OPEN_CORE_VOLTAGE).await?;
        self.enable_voltage().await?;

        // Voltage controller successfully initialized at this point, we should start sending
        // heart beats to it. Otherwise, it would shut down in about 10 seconds.
        self.start_heart_beat_task(halt_receiver).await;

        Ok(())
    }

    /// Helper method that sends heartbeat to the voltage controller at regular intervals
    ///
    /// The reason is to notify the voltage controller that we are alive so that it wouldn't
    /// cut-off power supply to the hashing chips on the board.
    async fn start_heart_beat_task(self: Arc<Self>, halt_receiver: halt::Receiver) {
        // Start heartbeat thread in termination context
        let voltage_ctrl = self.clone();
        halt_receiver
            .register_client("power heartbeat".into())
            .await
            .spawn(async move {
                loop {
                    voltage_ctrl
                        .send_heart_beat()
                        .await
                        .expect("send_heart_beat failed");
                    delay_for(VOLTAGE_CTRL_HEART_BEAT_PERIOD).await;
                }
            });

        // Make a termination handler that disables voltage when stopped
        let voltage_ctrl = self.clone();
        halt_receiver
            .register_client("power heartbeat termination".into())
            .await
            .spawn_halt_handler(async move {
                info!("Disabling voltage");
                voltage_ctrl
                    .disable_voltage()
                    .await
                    .expect("failed disabling voltage");
            });
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_pic_address_words() {
        let a = PicAddress(0x300);
        let b = PicAddress(0xffff);
        assert_eq!(a.offset(PicWords(0x500)), PicAddress(0x800));
        assert_eq!(a.distance_to(b), PicWords(0xfd00));
        assert_eq!(PicWords(0xf000).to_bytes(), 0x1e000);
        assert_eq!(PicWords::from_bytes(0x1fffe), PicWords(0xffff));
    }

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
