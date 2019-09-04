pub mod work_generation;

use super::*;

#[test]
fn test_midstate_count_instance() {
    MidstateCount::new(1);
    MidstateCount::new(2);
    MidstateCount::new(4);
}

#[test]
#[should_panic]
fn test_midstate_count_instance_fail() {
    MidstateCount::new(3);
}

#[test]
fn test_midstate_count_conversion() {
    use ii_fpga_io_am1_s9::hchainio0::ctrl_reg::MIDSTATE_CNT_A;

    assert_eq!(MidstateCount::new(4).to_mask(), 3);
    assert_eq!(MidstateCount::new(2).to_count(), 2);
    assert_eq!(MidstateCount::new(4).to_reg(), MIDSTATE_CNT_A::FOUR);
}

#[test]
fn test_hchain_ctl_instance() {
    let gpio_mgr = gpio::ControlPinManager::new();
    let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::new(0);
    let h_chain_ctl = HChainCtl::new(
        &gpio_mgr,
        voltage_ctrl_backend,
        config::S9_HASHBOARD_INDEX,
        MidstateCount::new(1),
        config::ASIC_DIFFICULTY,
    );
    match h_chain_ctl {
        Ok(_) => assert!(true),
        Err(e) => assert!(false, "Failed to instantiate hash chain, error: {}", e),
    }
}

#[test]
fn test_hchain_ctl_init() {
    let gpio_mgr = gpio::ControlPinManager::new();
    let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::new(0);
    let mut h_chain_ctl = HChainCtl::new(
        &gpio_mgr,
        voltage_ctrl_backend,
        config::S9_HASHBOARD_INDEX,
        MidstateCount::new(1),
        config::ASIC_DIFFICULTY,
    )
    .expect("Failed to create hash board instance");

    assert!(
        h_chain_ctl.ip_core_init().is_ok(),
        "Failed to initialize IP core"
    );

    // verify sane register values
    assert_eq!(
        h_chain_ctl.cmd_fifo.hash_chain_io.work_time.read().bits(),
        36296,
        "Unexpected work time value"
    );
    assert_eq!(
        h_chain_ctl.cmd_fifo.hash_chain_io.baud_reg.read().bits(),
        0x1a,
        "Unexpected baud rate register value for {} baud",
        INIT_CHIP_BAUD_RATE
    );
    assert_eq!(
        h_chain_ctl.cmd_fifo.hash_chain_io.stat_reg.read().bits(),
        0x855,
        "Unexpected status register value"
    );
    assert!(
        h_chain_ctl
            .cmd_fifo
            .hash_chain_io
            .ctrl_reg
            .read()
            .midstate_cnt()
            .is_one(),
        "Unexpected midstate count"
    );
}

/// This test verifies correct parsing of mining work solution for all multi-midstate
/// configurations.
/// The solution_word represents the second word of data provided that follows the nonce as
/// provided by the FPGA IP core
#[test]
fn test_get_solution_word_attributes() {
    let solution_word = 0x98123502;
    struct ExpectedSolutionData {
        work_id: usize,
        midstate_idx: usize,
        solution_idx: usize,
        midstate_count: MidstateCount,
    };
    let expected_solution_data = [
        ExpectedSolutionData {
            work_id: 0x1235,
            midstate_idx: 0,
            solution_idx: 2,
            midstate_count: MidstateCount::new(1),
        },
        ExpectedSolutionData {
            work_id: 0x1234,
            midstate_idx: 1,
            solution_idx: 2,
            midstate_count: MidstateCount::new(2),
        },
        ExpectedSolutionData {
            work_id: 0x1234,
            midstate_idx: 1,
            solution_idx: 2,
            midstate_count: MidstateCount::new(4),
        },
    ];
    for (i, expected_solution_data) in expected_solution_data.iter().enumerate() {
        // The midstate configuration (ctrl_reg::MIDSTATE_CNT_W) doesn't implement a debug
        // trait. Therefore, we extract only those parts that can be easily displayed when a
        // test failed.
        let expected_data = (
            expected_solution_data.work_id,
            expected_solution_data.midstate_idx,
            expected_solution_data.solution_idx,
        );
        let solution_id =
            SolutionId::from_reg(solution_word, expected_solution_data.midstate_count);

        assert_eq!(
            solution_id.work_id, expected_solution_data.work_id,
            "Invalid work ID, iteration: {}, test data: {:#06x?}",
            i, expected_data
        );
        assert_eq!(
            solution_id.midstate_idx, expected_solution_data.midstate_idx,
            "Invalid midstate index, iteration: {}, test data: {:#06x?}",
            i, expected_data
        );
        assert_eq!(
            solution_id.solution_idx, expected_solution_data.solution_idx,
            "Invalid solution index, iteration: {}, test data: {:#06x?}",
            i, expected_data
        );
    }
}
#[test]
fn test_calc_baud_div_correct_baud_rate_bm1387() {
    // these are sample baud rates for communicating with BM1387 chips
    let correct_bauds_and_divs = [
        (115_200usize, 26usize),
        (460_800, 6),
        (1_500_000, 1),
        (3_000_000, 0),
    ];
    for (baud_rate, baud_div) in correct_bauds_and_divs.iter() {
        let (baud_clock_div, actual_baud_rate) = calc_baud_clock_div(
            *baud_rate,
            CHIP_OSC_CLK_HZ,
            bm1387::CHIP_OSC_CLK_BASE_BAUD_DIV,
        )
        .unwrap();
        assert_eq!(
            baud_clock_div, *baud_div,
            "Calculated baud divisor doesn't match, requested: {} baud, actual: {} baud",
            baud_rate, actual_baud_rate
        )
    }
}

/// Test higher baud rate than supported
#[test]
fn test_calc_baud_div_over_baud_rate_bm1387() {
    let result = calc_baud_clock_div(
        3_500_000,
        CHIP_OSC_CLK_HZ,
        bm1387::CHIP_OSC_CLK_BASE_BAUD_DIV,
    );
    assert!(
        result.is_err(),
        "Baud clock divisor unexpectedly calculated!"
    );
}

/// Test work_time computation
#[test]
fn test_work_time_computation() {
    // you need to recalc this if you change asic diff or fpga freq
    assert_eq!(
        secs_to_fpga_ticks(calculate_work_delay_for_pll(1, 650_000_000)),
        36296
    );
}

#[test]
fn test_work_id_gen() {
    let mut work_id_gen = WorkIdGen::new(MidstateCount::new(2));
    assert_eq!(work_id_gen.next(), 0);
    assert_eq!(work_id_gen.next(), 2);
    assert_eq!(work_id_gen.next(), 4);
    work_id_gen.work_id = 0xfffe;
    assert_eq!(work_id_gen.next(), 0xfffe);
    assert_eq!(work_id_gen.next(), 0);
}
