extern crate linux_embedded_hal;
extern crate rminer;
extern crate s9_io;
extern crate uint;

use rminer::hal;
use rminer::hal::s9::gpio;
use rminer::hal::s9::power;
use rminer::hal::HardwareCtl;

use linux_embedded_hal::I2cdev;

use std::thread;
use std::time::Duration;

/// Prepares sample work with empty midstates
/// NOTE: this work has 2 valid nonces:
/// - 0x83ea0372 (solution 0)
/// - 0x09f86be1 (solution 1)
fn prepare_test_work() -> hal::MiningWork {
    hal::MiningWork {
        version: 0,
        extranonce_2: 0,
        midstates: vec![uint::U256([0, 0, 0, 0])],
        merkel_root_lsw: 0xffff_ffff,
        nbits: 0xffff_ffff,
        ntime: 0xffff_ffff,
    }
}

///
/// * `work_count` - number of work items to generate
/// * `expected_result_count` - number of expected solutions that the hash chain should provide
fn send_and_receive_test_workloads<T>(
    h_chain_ctl: &mut hal::s9::HChainCtl<T>,
    work_count: usize,
    expected_result_count: usize,
) where
    T: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    use hal::HardwareCtl;

    let mut work_result_count = 0usize;

    for i in 0..work_count {
        let test_work = prepare_test_work();
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

/// Verifies work generation for a hash chain
///
/// The test runs two batches of work:
/// - the first 3 work items are for initializing input queues of the chips and result in no
/// action (no solutions are provided
/// - the next 2 work items yield actual solutions. Since we don't push more work items, the
/// solution 1 never appears on the bus and leave chips output queues. This is fine as this test
/// is intended for initial check of correct operation
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
    )
    .unwrap();

    h_chain_ctl.init().unwrap();

    // the first 3 work loads don't produce any results, these are merely to initialize the input
    // queue of each hashing chip
    send_and_receive_test_workloads(&mut h_chain_ctl, 3, 0);
    // submit 2 more work items, since we are intentionally being slow all chips should send a
    // result for the submitted work
    let more_work_count = 2usize;
    let expected_result_count = more_work_count * h_chain_ctl.get_chip_count();
    send_and_receive_test_workloads(&mut h_chain_ctl, more_work_count, expected_result_count);
}
