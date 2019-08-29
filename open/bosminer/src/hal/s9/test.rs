use super::*;
use s9_io::hchainio0;

pub mod work_generation;

#[test]
fn test_hchain_ctl_instance() {
    let gpio_mgr = gpio::ControlPinManager::new();
    let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::new(0);
    let h_chain_ctl = HChainCtl::new(
        &gpio_mgr,
        voltage_ctrl_backend,
        config::S9_HASHBOARD_INDEX,
        hchainio0::ctrl_reg::MIDSTATE_CNT_A::ONE,
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
        hchainio0::ctrl_reg::MIDSTATE_CNT_A::ONE,
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
    let solution_word = 0x00123502;
    struct ExpectedSolutionData {
        work_id: u32,
        midstate_idx: usize,
        solution_idx: usize,
        midstate_count_log2: hchainio0::ctrl_reg::MIDSTATE_CNT_A,
    };
    let expected_solution_data = [
        ExpectedSolutionData {
            work_id: 0x1235,
            midstate_idx: 0,
            solution_idx: 2,
            midstate_count_log2: hchainio0::ctrl_reg::MIDSTATE_CNT_A::ONE,
        },
        ExpectedSolutionData {
            work_id: 0x1234,
            midstate_idx: 1,
            solution_idx: 2,
            midstate_count_log2: hchainio0::ctrl_reg::MIDSTATE_CNT_A::TWO,
        },
        ExpectedSolutionData {
            work_id: 0x1234,
            midstate_idx: 1,
            solution_idx: 2,
            midstate_count_log2: hchainio0::ctrl_reg::MIDSTATE_CNT_A::FOUR,
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
        let gpio_mgr = gpio::ControlPinManager::new();
        let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::new(0);
        let h_chain_ctl = HChainCtl::new(
            &gpio_mgr,
            voltage_ctrl_backend,
            config::S9_HASHBOARD_INDEX,
            expected_solution_data.midstate_count_log2,
            config::ASIC_DIFFICULTY,
        )
        .unwrap();

        assert_eq!(
            h_chain_ctl.get_work_id_from_solution_id(solution_word),
            expected_solution_data.work_id,
            "Invalid work ID, iteration: {}, test data: {:#06x?}",
            i,
            expected_data
        );
        assert_eq!(
            h_chain_ctl.get_midstate_idx_from_solution_id(solution_word),
            expected_solution_data.midstate_idx,
            "Invalid midstate index, iteration: {}, test data: {:#06x?}",
            i,
            expected_data
        );
        assert_eq!(
            h_chain_ctl.get_solution_idx_from_solution_id(solution_word),
            expected_solution_data.solution_idx,
            "Invalid solution index, iteration: {}, test data: {:#06x?}",
            i,
            expected_data
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
