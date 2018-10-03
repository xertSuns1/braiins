extern crate embedded_hal;
extern crate linux_embedded_hal;
extern crate rminer;

use rminer::hal::s9::gpio;
use rminer::hal::s9::power;

use embedded_hal::digital::InputPin;
use embedded_hal::digital::OutputPin;
use linux_embedded_hal::I2cdev;

/// Helper function that tests 1 hashchain
/// * `ctrl_pin_manager` - provides accesss to  GPIO control pins connected to the hashboard
/// * `idx` - index of the hashboard
fn test_voltage_ctrl_on_1_hashboard(idx: usize, ctrl_pin_manager: &gpio::ControlPinManager) {
    let mut reset = ctrl_pin_manager
        .get_pin_out(gpio::PinOutName::Rst(idx))
        .unwrap();

    // perform reset of the hashboard
    reset.set_low();
    reset.set_high();

    let mut backend = power::VoltageCtrlI2cBlockingBackend::<I2cdev>::new(0);
    let mut voltage_ctrl = power::VoltageCtrl::new(&mut backend, idx);

    voltage_ctrl.reset().unwrap();
    voltage_ctrl.jump_from_loader_to_app().unwrap();

    let version = voltage_ctrl.get_version().unwrap();
    let expected_version = 3;
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

        if plug.is_high() {
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
