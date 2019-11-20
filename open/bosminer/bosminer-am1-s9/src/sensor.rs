// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU Common Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Common Public License for more details.
//
// You should have received a copy of the GNU Common Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

//! This module contains interface for reading from sensor (`Sensor`) and what
//! constitutes a sensor reading (`Temperature`, `Measurement`).

pub mod tmp451;

use crate::error;
use crate::i2c;

use async_trait::async_trait;
use ii_logging::macros::*;
use lazy_static::lazy_static;
use std::boxed::Box;

/// Generic sensor
#[async_trait]
pub trait Sensor: Sync + Send {
    /// Initialize the sensor (should be called at least once before first call to `read_temperature`
    async fn init(&mut self) -> error::Result<()>;

    /// Read temperature from sensor
    async fn read_temperature(&mut self) -> error::Result<Temperature>;
}

/// Result of measuring temperature with remote sensor
#[derive(Debug, PartialEq, Clone)]
pub enum Measurement {
    /// Sensor not present
    NotPresent,
    /// Sensor broke off
    OpenCircuit,
    /// Sensor is "shorted"
    ShortCircuit,
    /// OK, temperature in degree celsius
    Ok(f32),
}

/// Temperature reading
#[derive(Debug, PartialEq, Clone)]
pub struct Temperature {
    /// Local temperature (internal to the sensor) - usually present
    pub local: Measurement,

    /// Remote aka external sensor - may fail or not be present at all
    pub remote: Measurement,
}

lazy_static! {
    /// List of all known I2C address where sensors are present
    static ref SENSOR_I2C_ADDRESS: [i2c::Address; 3] = [
        i2c::Address::new(0x98),
        i2c::Address::new(0x9a),
        i2c::Address::new(0x9c),
    ];
}

/// Probe one I2C address for known sensor
///
/// This is pretty much ad-hoc function that doesn't utilize the `inventory` of our I2C sensor
/// drivers. The logic for detecting the type of I2C temp sensor is pretty much random (see for
/// example `lm90` driver in Linux kernel), so just don't bother with a generic detection
/// algorithm.
pub async fn probe_i2c_device(
    mut i2c_device: Box<dyn i2c::AsyncDevice>,
) -> error::Result<Option<Box<dyn Sensor>>> {
    // Interesting SMBus registers
    const REG_MANUFACTURER_ID: u8 = 0xfe;
    const REG_DEVICE_ID: u8 = 0xff;

    // Read manufacturer and device ID
    let manufacturer_id = i2c_device.read(REG_MANUFACTURER_ID).await?;
    let device_id = i2c_device.read(REG_DEVICE_ID).await?;

    info!(
        "{:?} manufacturer_id={:#x} device_id={:#x}",
        i2c_device.get_address(),
        manufacturer_id,
        device_id
    );

    let sensor = match manufacturer_id {
        0x55 => Some(tmp451::TMP451::new(i2c_device)),
        0x41 => Some(tmp451::ADT7461::new(i2c_device)),
        0x1a => Some(tmp451::NCT218::new(i2c_device)),
        _ => None,
    };

    Ok(sensor)
}

/// Probe for known sensors
pub async fn probe_i2c_sensors<T: 'static + i2c::AsyncBus + Clone>(
    i2c_bus: T,
) -> error::Result<Option<Box<dyn Sensor>>> {
    // Go through all known addresses
    for address in SENSOR_I2C_ADDRESS.iter() {
        // Construct device at given i2c address
        let i2c_device = Box::new(i2c::Device::new(i2c_bus.clone(), *address));

        // Try to probe this device
        match probe_i2c_device(i2c_device).await? {
            sensor @ Some(_) => return Ok(sensor),
            _ => (),
        }
    }

    // No sensors were found
    Ok(None)
}

#[cfg(test)]
mod test {
    use super::*;
    use i2c::test_utils;
    use ii_async_compat::tokio;

    async fn test_probe_address(addr: u8, man_id: u8) -> bool {
        let bus = test_utils::FakeI2cBus::new(
            i2c::Address::new(addr),
            &[test_utils::InitReg(0xfe, man_id)],
            Some(0),
            Some(0xff),
        );
        let bus = i2c::SharedBus::new(bus);
        let result = probe_i2c_sensors(bus).await.unwrap();
        result.is_some()
    }

    #[tokio::test]
    async fn inner_test_probe_i2c_sensors() {
        assert_eq!(test_probe_address(0x98, 0x55).await, true);
        assert_eq!(test_probe_address(0x9a, 0x41).await, true);
        assert_eq!(test_probe_address(0x9c, 0x1a).await, true);
        assert_eq!(test_probe_address(0x9c, 0x37).await, false);
        assert_eq!(test_probe_address(0x84, 0x55).await, false);
    }
}
