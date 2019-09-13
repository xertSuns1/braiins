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

use bosminer_am1_s9::gpio;
use bosminer_am1_s9::power;

use embedded_hal::digital::v2::InputPin;
use embedded_hal::digital::v2::OutputPin;

/// Helper function that tests voltage controller on a particular hashboard.
///
/// The test simply verifies that the voltage controller responds with a valid version
///
/// * `ctrl_pin_manager` - provides accesss to  GPIO control pins connected to the hashboard
/// * `idx` - index of the hashboard
fn test_voltage_ctrl_on_1_hashboard(idx: usize, ctrl_pin_manager: &gpio::ControlPinManager) {
    let mut reset = ctrl_pin_manager
        .get_pin_out(gpio::PinOutName::Rst(idx))
        .unwrap();

    // perform reset of the hashboard
    reset.set_low().unwrap();
    reset.set_high().unwrap();

    let backend = power::I2cBackend::new(0);
    let backend = power::SharedBackend::new(backend);
    let mut voltage_ctrl = power::Control::new(backend, idx);

    voltage_ctrl.reset().unwrap();
    voltage_ctrl.jump_from_loader_to_app().unwrap();

    let version = voltage_ctrl.get_version().unwrap();
    let expected_version: u8 = 3;
    assert_eq!(
        version, expected_version,
        "Expected version {:x}",
        expected_version
    );
}

/// Attempts to run voltage controller test for all hashboards. A minimum of one hashboard is
/// required to be present in the miner
#[test]
fn test_voltage_ctrl_all_hashboards() {
    let ctrl_pin_manager = gpio::ControlPinManager::new();
    let mut tested_hashboards: usize = 0;
    let expected_tested_hashboards: usize = 1;

    for hashboard_idx in 1..9 {
        let plug = ctrl_pin_manager
            .get_pin_in(gpio::PinInName::Plug(hashboard_idx))
            .unwrap();

        if plug.is_high().unwrap() {
            test_voltage_ctrl_on_1_hashboard(hashboard_idx, &ctrl_pin_manager);
            tested_hashboards += 1;
        }
    }
    assert!(
        tested_hashboards >= expected_tested_hashboards,
        "Not enough hashboards found, cannot test voltage controller, tested: {} expected:{}",
        tested_hashboards,
        expected_tested_hashboards
    );
}
