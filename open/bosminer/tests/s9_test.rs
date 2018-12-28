extern crate linux_embedded_hal;
extern crate rminer;
extern crate s9_io;
extern crate uint;

use rminer::hal;
use rminer::hal::s9::gpio;
use rminer::hal::s9::power;
use rminer::hal::HardwareCtl;

use linux_embedded_hal::I2cdev;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// * `_i` - unique identifier that makes the work distinct
fn prepare_test_work(_i: usize) -> hal::MiningWork {
    hal::MiningWork {
        version: 0,
        extranonce_2: 0,
        midstates: vec![uint::U256([0, 0, 0, 0])],
        merkel_root_lsw: 0xffff_ffff,
        nbits: 0xffff_ffff,
        ntime: 0xffff_ffff,
        //            version: 0,
        //            extranonce_2: 0,
        //            midstates: vec![uint::U256([v, 2, 3, 4])],
        //            merkel_root_lsw: 0xdeadbeef,
        //            nbits: 0x1a44b9f2,
        //            ntime: 0x4dd7f5c7,
    }
}

/// * `work_start` - beginning of the unique test work range
/// * `end_start` - end of the unique test work range (excluded)
fn send_and_receive_test_workloads<T>(
    h_chain_ctl: &mut hal::s9::HChainCtl<T>,
    work_start: usize,
    work_end: usize,
    expected_result_count: usize,
) where
    T: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    use hal::HardwareCtl;

    let mut work_result_count = 0usize;

    for i in work_start..work_end {
        let test_work = prepare_test_work(i);
        let work_id = h_chain_ctl.send_work(&test_work).unwrap();
        // wait until the work is physically sent out it takes around 5 ms for the FPGA IP core
        // to send out the work @ 115.2 kBaud
        thread::sleep(Duration::from_millis(10));
        while let Some(work_result) = h_chain_ctl.recv_work_result().unwrap() {
            println!("Iteration:{}\n{:#010x?}", i, work_result);
            assert_eq!(
                work_id,
                h_chain_ctl.get_work_id_from_result(&work_result),
                "Unexpected work ID detected in returned mining result"
            );
            work_result_count += 1;
        }
    }
    assert_eq!(
        work_result_count, expected_result_count,
        "Unexpected number of work results"
    )
}

#[test]
fn test_work_generation() {
    use hal::s9::power::VoltageCtrlBackend;

    let gpio_mgr = gpio::ControlPinManager::new();
    let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::<I2cdev>::new(0);
    let voltage_ctrl_backend =
        power::VoltageCtrlI2cSharedBlockingBackend::new(voltage_ctrl_backend);
    let mut h_chain_ctl = hal::s9::HChainCtl::new(
        &gpio_mgr,
        voltage_ctrl_backend.clone(),
        8,
        &s9_io::hchainio0::ctrl_reg::MIDSTATE_CNTW::ONE,
    ).unwrap();

    h_chain_ctl.init().unwrap();

    // the first 3 work loads don't produce any results, these are merely to initialize the input
    // queue of each hashing chip
    send_and_receive_test_workloads(&mut h_chain_ctl, 0, 3, 0);
    // submit 2 more workloads, these are expected to yield the same results from every single chip
    let expected_result_count = 2 * h_chain_ctl.get_chip_count();
    send_and_receive_test_workloads(&mut h_chain_ctl, 3, 5, expected_result_count);
}
