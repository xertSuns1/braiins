extern crate linux_embedded_hal;
extern crate rminer;
extern crate s9_io;
extern crate uint;

use rminer::hal;
use rminer::hal::s9::gpio;
use rminer::hal::s9::power;
use rminer::hal::HardwareCtl;

use linux_embedded_hal::I2cdev;

#[test]
fn test_work_generation() {
    let gpio_mgr = gpio::ControlPinManager::new();
    let mut voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::<I2cdev>::new(0);
    let mut h_chain_ctl = hal::s9::HChainCtl::new(
        &gpio_mgr,
        &mut voltage_ctrl_backend,
        8,
        &s9_io::hchainio0::ctrl_reg::MIDSTATE_CNTW::ONE,
    ).unwrap();

    h_chain_ctl.init().unwrap();
    let mut result_count = 0usize;

    for v in 0..6 {
        let test_work = hal::MiningWork {
            version: 0,
            extranonce_2: 0,
            midstates: vec![uint::U256([0, 0, 0, 0])],
            merkel_root_lsw: 0xffff_ffff,
            nbits: 0xffff_ffff,
            ntime: 0xffff_ffff,
            //            version: 0,
            //            extranonce_2: 0,
            //            midstates: vec![uint::U256([v, 2, 3, 4])]
            //            merkel_root_lsw: 0xdeadbeef,
            //            nbits: 0x1a44b9f2,
            //            ntime: 0x4dd7f5c7,
        };

        h_chain_ctl.send_work(&test_work);

        match h_chain_ctl.recv_work_result().unwrap() {
            Some(work_result) => {
                println!("{:#010x?}", work_result);
                result_count += 1;
            }
            None => println!("No new result"),
        }
    }

    while true {}
}
