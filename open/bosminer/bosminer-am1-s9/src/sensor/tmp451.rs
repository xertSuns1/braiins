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

//! Driver implementation of sensor driver for TMP451 and similar sensors

use crate::error;
use crate::i2c;
use crate::sensor::{self, I2cSensorDriver, Measurement, Temperature};

use async_trait::async_trait;
use std::boxed::Box;

const REG_LOCAL_TEMP: u8 = 0x00;
const REG_REMOTE_TEMP: u8 = 0x01;
const REG_STATUS: u8 = 0x02;
const STATUS_OPEN_CIRCUIT: u8 = 0x04;
const REG_CONFIG: u8 = 0x03;
const REG_CONFIG_W: u8 = 0x09;
const CONFIG_RANGE: u8 = 0x04;
const REG_OFFSET: u8 = 0x11;
const REG_REMOTE_FRAC_TEMP: u8 = 0x10;
const REG_LOCAL_FRAC_TEMP: u8 = 0x15;

/// Build a temperature from internal representation
fn make_temp(whole: u8, fract: u8) -> f32 {
    (whole as f32 - 64.0) + (fract as f32 / 256.0)
}

/// Read both local and remote temperatures.
/// Check if external sensor is working properly.
///
/// * `use_fract` - determines if we read and interpret the fractional part of
///   temperature.
///   It makes sense even for sensors that are precise +- 1 degree (because they
///   have internal filtering.
async fn read_temperature(
    i2c_dev: &mut Box<dyn i2c::AsyncDevice>,
    use_fract: bool,
) -> error::Result<Temperature> {
    let status = await!(i2c_dev.read(REG_STATUS))?;
    let local_temp = await!(i2c_dev.read(REG_LOCAL_TEMP))?;
    let local_frac = if use_fract {
        await!(i2c_dev.read(REG_LOCAL_FRAC_TEMP))?
    } else {
        0
    };
    let remote_temp = await!(i2c_dev.read(REG_REMOTE_TEMP))?;
    let remote_frac = if use_fract {
        await!(i2c_dev.read(REG_REMOTE_FRAC_TEMP))?
    } else {
        0
    };

    let local = make_temp(local_temp, local_frac);
    let remote;
    if (status & STATUS_OPEN_CIRCUIT) != 0 {
        remote = Measurement::OpenCircuit;
    } else if remote_temp == 0 {
        remote = Measurement::ShortCircuit;
    } else {
        remote = Measurement::Ok(make_temp(remote_temp, remote_frac))
    };

    Ok(Temperature { local, remote })
}

/// Read only local temperature
async fn read_temperature_local(
    i2c_dev: &mut Box<dyn i2c::AsyncDevice>,
) -> error::Result<Temperature> {
    let local_temp = await!(i2c_dev.read(REG_LOCAL_TEMP))?;

    Ok(Temperature {
        local: make_temp(local_temp, 0),
        remote: Measurement::NotPresent,
    })
}

async fn generic_init(i2c_dev: &mut Box<dyn i2c::AsyncDevice>) -> error::Result<()> {
    await!(i2c_dev.write_readback(REG_CONFIG_W, REG_CONFIG, CONFIG_RANGE))?;
    await!(i2c_dev.write(REG_OFFSET, 0))?;
    Ok(())
}

/// TMP451 driver (most common type, has remote sensor)
pub struct TMP451 {
    i2c_dev: Box<dyn i2c::AsyncDevice>,
}

#[async_trait]
impl sensor::Sensor for TMP451 {
    async fn init(&mut self) -> error::Result<()> {
        await!(generic_init(&mut self.i2c_dev))
    }

    async fn read_temperature(&mut self) -> error::Result<Temperature> {
        await!(read_temperature(&mut self.i2c_dev, true))
    }
}

impl sensor::I2cSensor for TMP451 {
    const MANUFACTURER_ID: u8 = 0x55;

    fn new(i2c_dev: Box<dyn i2c::AsyncDevice>) -> Self {
        Self { i2c_dev }
    }
}

inventory::submit! {
    I2cSensorDriver::new::<TMP451>()
}

/// ADT7461 driver (almost the same as TMP451)
pub struct ADT7461 {
    i2c_dev: Box<dyn i2c::AsyncDevice>,
}

#[async_trait]
impl sensor::Sensor for ADT7461 {
    async fn init(&mut self) -> error::Result<()> {
        await!(generic_init(&mut self.i2c_dev))
    }

    async fn read_temperature(&mut self) -> error::Result<Temperature> {
        await!(read_temperature(&mut self.i2c_dev, false))
    }
}

impl sensor::I2cSensor for ADT7461 {
    const MANUFACTURER_ID: u8 = 0x41;

    fn new(i2c_dev: Box<dyn i2c::AsyncDevice>) -> Self {
        Self { i2c_dev }
    }
}

inventory::submit! {
    I2cSensorDriver::new::<ADT7461>()
}

/// NCT218 driver (only local temperature)
pub struct NCT218 {
    i2c_dev: Box<dyn i2c::AsyncDevice>,
}

#[async_trait]
impl sensor::Sensor for NCT218 {
    async fn init(&mut self) -> error::Result<()> {
        await!(generic_init(&mut self.i2c_dev))
    }

    async fn read_temperature(&mut self) -> error::Result<Temperature> {
        await!(read_temperature_local(&mut self.i2c_dev))
    }
}

impl sensor::I2cSensor for NCT218 {
    const MANUFACTURER_ID: u8 = 0x1a;

    fn new(i2c_dev: Box<dyn i2c::AsyncDevice>) -> Self {
        Self { i2c_dev }
    }
}

inventory::submit! {
    I2cSensorDriver::new::<NCT218>()
}

#[cfg(test)]
mod test {
    use super::*;
    use i2c::test_utils::InitReg;

    /// Make sensor T with data being read/written from memory `data`
    fn make_sensor<T: 'static + sensor::Sensor + sensor::I2cSensor>(
        data: &[InitReg],
    ) -> (Box<dyn sensor::Sensor>, Box<dyn i2c::AsyncDevice>) {
        let addr = i2c::Address::new(0x16);
        // poison all registers except those we define
        let bus = i2c::test_utils::FakeI2cBus::new(addr, data, None, None);
        let bus = i2c::SharedBus::new(bus);
        let dev = i2c::Device::new(bus, addr);
        let driver = T::new(Box::new(dev.clone()));

        (Box::new(driver) as Box<dyn sensor::Sensor>, Box::new(dev))
    }

    async fn check_config_ok(dev: &mut Box<dyn i2c::AsyncDevice>) {
        assert_eq!(
            await!(dev.read(REG_CONFIG_W)).unwrap() & CONFIG_RANGE,
            CONFIG_RANGE
        );
        assert_eq!(await!(dev.read(REG_OFFSET)).unwrap(), 0);
    }

    async fn inner_test_sensor_drivers_i2c() {
        let ok_regs = [
            // 23 deg
            InitReg(REG_LOCAL_TEMP, 0x57),
            // 41 deg
            InitReg(REG_REMOTE_TEMP, 0x69),
            InitReg(REG_STATUS, 0x00),
            // .1875 deg
            InitReg(REG_LOCAL_FRAC_TEMP, 0x30),
            // .2500 deg
            InitReg(REG_REMOTE_FRAC_TEMP, 0x40),
            // Config range (this is a little bit of a hack: we pre-set
            // this value so that `write_readback` in driver succeeds.
            InitReg(REG_CONFIG, 0x04),
            // Config range write
            InitReg(REG_CONFIG_W, 0x00),
            // Config offset to 0
            InitReg(REG_OFFSET, 0x7f),
        ];

        // Check "working conditions" on TMP451
        let (mut sensor, mut dev) = make_sensor::<TMP451>(&ok_regs);
        await!(sensor.init()).unwrap();
        await!(check_config_ok(&mut dev));
        assert_eq!(
            await!(sensor.read_temperature()).unwrap(),
            Temperature {
                local: 23.1875,
                remote: Measurement::Ok(41.25),
            }
        );

        // Check "working conditions" on ADT7461
        let (mut sensor, mut dev) = make_sensor::<ADT7461>(&ok_regs);
        await!(sensor.init()).unwrap();
        await!(check_config_ok(&mut dev));
        assert_eq!(
            await!(sensor.read_temperature()).unwrap(),
            Temperature {
                local: 23.0,
                remote: Measurement::Ok(41.0),
            }
        );

        // Check "working conditions" on NCT218
        let (mut sensor, mut dev) = make_sensor::<NCT218>(&ok_regs);
        await!(sensor.init()).unwrap();
        await!(check_config_ok(&mut dev));
        assert_eq!(
            await!(sensor.read_temperature()).unwrap(),
            Temperature {
                local: 23.0,
                remote: Measurement::NotPresent,
            }
        );
    }

    #[test]
    fn test_sensor_drivers_i2c() {
        ii_async_compat::run_main_exits(async {
            await!(inner_test_sensor_drivers_i2c());
        });
    }

    async fn inner_test_sensor_drivers_i2c_open_circuit() {
        let ok_regs = [
            // 23 deg
            InitReg(REG_LOCAL_TEMP, 0x57),
            // 41 deg
            InitReg(REG_REMOTE_TEMP, 0x69),
            // external sensor is broken-off
            InitReg(REG_STATUS, STATUS_OPEN_CIRCUIT),
            // .1875 deg
            InitReg(REG_LOCAL_FRAC_TEMP, 0x30),
            // .2500 deg
            InitReg(REG_REMOTE_FRAC_TEMP, 0x40),
            // Config range (this is a little bit of a hack: we pre-set
            // this value so that `write_readback` in driver succeeds.
            InitReg(REG_CONFIG, 0x04),
            // Config range write
            InitReg(REG_CONFIG_W, 0x00),
            // Config offset to 0
            InitReg(REG_OFFSET, 0x7f),
        ];

        // Test TMP451
        let (mut sensor, mut dev) = make_sensor::<TMP451>(&ok_regs);
        await!(sensor.init()).unwrap();
        await!(check_config_ok(&mut dev));
        assert_eq!(
            await!(sensor.read_temperature()).unwrap(),
            Temperature {
                local: 23.1875,
                remote: Measurement::OpenCircuit,
            }
        );

        // Test ADT7461
        let (mut sensor, mut dev) = make_sensor::<ADT7461>(&ok_regs);
        await!(sensor.init()).unwrap();
        await!(check_config_ok(&mut dev));
        assert_eq!(
            await!(sensor.read_temperature()).unwrap(),
            Temperature {
                local: 23.0,
                remote: Measurement::OpenCircuit,
            }
        );
    }

    #[test]
    fn test_sensor_drivers_i2c_open_circuit() {
        ii_async_compat::run_main_exits(async {
            await!(inner_test_sensor_drivers_i2c_open_circuit());
        });
    }

    async fn inner_test_sensor_drivers_i2c_short_circuit() {
        let ok_regs = [
            // 23 deg
            InitReg(REG_LOCAL_TEMP, 0x57),
            // short-circuit
            InitReg(REG_REMOTE_TEMP, 0x00),
            InitReg(REG_STATUS, 0),
            // .1875 deg
            InitReg(REG_LOCAL_FRAC_TEMP, 0x30),
            // .0000 deg
            InitReg(REG_REMOTE_FRAC_TEMP, 0x00),
            // Config range (this is a little bit of a hack: we pre-set
            // this value so that `write_readback` in driver succeeds.
            InitReg(REG_CONFIG, 0x04),
            // Config range write
            InitReg(REG_CONFIG_W, 0x00),
            // Config offset to 0
            InitReg(REG_OFFSET, 0x7f),
        ];

        // Test TMP451
        let (mut sensor, mut dev) = make_sensor::<TMP451>(&ok_regs);
        await!(sensor.init()).unwrap();
        await!(check_config_ok(&mut dev));
        assert_eq!(
            await!(sensor.read_temperature()).unwrap(),
            Temperature {
                local: 23.1875,
                remote: Measurement::ShortCircuit,
            }
        );

        // Test ADT7461
        let (mut sensor, mut dev) = make_sensor::<ADT7461>(&ok_regs);
        await!(sensor.init()).unwrap();
        await!(check_config_ok(&mut dev));
        assert_eq!(
            await!(sensor.read_temperature()).unwrap(),
            Temperature {
                local: 23.0,
                remote: Measurement::ShortCircuit,
            }
        );
    }

    #[test]
    fn test_sensor_drivers_i2c_short_circuit() {
        ii_async_compat::run_main_exits(async {
            await!(inner_test_sensor_drivers_i2c_short_circuit());
        });
    }
}
