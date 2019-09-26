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
//!
//! This module also collects all i2c sensor drivers (using inventory on
//! `I2cSensorDriver`) and knows how to probe them if given an I2c bus.

pub mod tmp451;

use crate::error;
use crate::i2c::{self, AsyncDevice};

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

/// I2C sensor
pub trait I2cSensor: Sensor {
    /// Value that has to match manufacturer ID register
    const MANUFACTURER_ID: u8;

    /// Function to build this sensor
    fn new(i2c_dev: Box<dyn i2c::AsyncDevice>) -> Self;
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
    /// Local temperature is always present
    local: f32,

    /// Remote aka external sensor - may fail or not be present at all
    remote: Measurement,
}

/// Definition of a (i2c) driver that can construct (i2c) sensors
pub struct I2cSensorDriver {
    /// We distinguish sensors by this number
    manufacturer_id: u8,
    /// Function to construct new boxed sensor
    new: &'static dyn Fn(Box<dyn i2c::AsyncDevice>) -> Box<dyn Sensor>,
}

impl I2cSensorDriver {
    /// Aux function to register new driver
    pub fn new<T: 'static + I2cSensor>() -> Self {
        Self {
            manufacturer_id: T::MANUFACTURER_ID,
            new: &|i2c_dev| Box::new(T::new(i2c_dev)),
        }
    }
}
inventory::collect!(I2cSensorDriver);

lazy_static! {
    /// List of all known I2C address where sensors are present
    static ref SENSOR_I2C_ADDRESS: [i2c::Address; 3] = [
        i2c::Address::new(0x98),
        i2c::Address::new(0x9a),
        i2c::Address::new(0x9c),
    ];
}

/// Probe for known sensors
pub async fn probe_i2c_sensors<T: 'static + i2c::AsyncBus + Clone>(
    i2c_bus: T,
) -> error::Result<Option<Box<dyn Sensor>>> {
    // These are addresses to be probed
    const REG_MANUFACTURER_ID: u8 = 0xfe;

    // Go through all known addresses
    for address in SENSOR_I2C_ADDRESS.iter() {
        // Construct device
        let mut i2c_device = i2c::Device::new(i2c_bus.clone(), *address);
        // Read manufacturer ID
        let manufacturer_id = await!(i2c_device.read(REG_MANUFACTURER_ID))?;

        info!("{:?} manufacturer_id={:#x}", address, manufacturer_id);

        // Lookup which drivers do support this manufacturer ID
        for driver in inventory::iter::<I2cSensorDriver> {
            if driver.manufacturer_id == manufacturer_id {
                return Ok(Some((driver.new)(Box::new(i2c_device))));
            }
        }
    }

    // No sensors were found
    Ok(None)
}

#[cfg(test)]
mod test {
    use super::*;
    use i2c::test_utils;

    async fn test_probe_address(addr: u8, man_id: u8) -> bool {
        let bus = test_utils::FakeI2cBus::new(
            i2c::Address::new(addr),
            &[test_utils::InitReg(0xfe, man_id)],
            Some(0),
            Some(0xff),
        );
        let bus = i2c::SharedBus::new(bus);
        let result = await!(probe_i2c_sensors(bus)).unwrap();
        result.is_some()
    }

    async fn inner_test_probe_i2c_sensors() {
        assert_eq!(await!(test_probe_address(0x98, 0x55)), true);
        assert_eq!(await!(test_probe_address(0x9a, 0x41)), true);
        assert_eq!(await!(test_probe_address(0x9c, 0x1a)), true);
        assert_eq!(await!(test_probe_address(0x9c, 0x37)), false);
        assert_eq!(await!(test_probe_address(0x84, 0x55)), false);
    }

    #[test]
    fn test_probe_i2c_sensors() {
        ii_async_compat::run_main_exits(async {
            await!(inner_test_probe_i2c_sensors());
        });
    }
}
