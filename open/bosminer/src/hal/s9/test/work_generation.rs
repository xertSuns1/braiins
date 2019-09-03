use ii_logging::macros::*;

use super::*;

use crate::hal;

use std::time::{Duration, Instant};

use futures_locks::Mutex;
use std::sync::Arc;

use futures::channel::mpsc;
use futures::stream::StreamExt;
use tokio::await;
use tokio::timer::Delay;

use ii_async_compat::{timeout_future, TimeoutResult};

/// Our local abbreviation
type HChainCtl = super::HChainCtl<
    power::VoltageCtrlI2cSharedBlockingBackend<power::VoltageCtrlI2cBlockingBackend>,
>;

/// Prepares sample work with empty midstates
/// NOTE: this work has 2 valid nonces:
/// - 0x83ea0372 (solution 0)
/// - 0x09f86be1 (solution 1)
fn prepare_test_work(midstate_count: usize) -> hal::MiningWork {
    let time = 0xffffffff;
    let job = Arc::new(null_work::NullJob::new(time, 0xffff_ffff, 0));

    let one_midstate = hal::Midstate {
        version: 0,
        state: [0u8; 32].into(),
    };
    hal::MiningWork::new(job, vec![one_midstate; midstate_count], time)
}

/// Task that receives solutions from hardware and sends them to channel
async fn receiver_task(
    h_chain_ctl: Arc<Mutex<HChainCtl>>,
    solution_sender: mpsc::UnboundedSender<hal::MiningWorkSolution>,
) {
    let mut rx_fifo = await!(h_chain_ctl.lock())
        .expect("locking failed")
        .work_rx_fifo
        .take()
        .expect("work-rx fifo missing");
    loop {
        let (rx_fifo_out, solution) =
            await!(HChainCtl::recv_solution(h_chain_ctl.clone(), rx_fifo)).expect("recv solution");
        rx_fifo = rx_fifo_out;

        let solution = solution.expect("solution is not OK");
        solution_sender
            .unbounded_send(solution)
            .expect("solution send failed");
    }
}

/// Task that receives work from channel and sends it to HW
async fn sender_task(
    h_chain_ctl: Arc<Mutex<HChainCtl>>,
    mut work_receiver: mpsc::UnboundedReceiver<hal::MiningWork>,
) {
    let mut tx_fifo = await!(h_chain_ctl.lock())
        .expect("locking failed")
        .work_tx_fifo
        .take()
        .expect("work-tx fifo missing");

    loop {
        await!(tx_fifo.async_wait_for_work_tx_room()).expect("wait for tx room");
        let work = await!(work_receiver.next()).expect("failed receiving work");
        let work_id = await!(h_chain_ctl.lock())
            .expect("h_chain lock")
            .next_work_id();
        // send work is synchronous
        tx_fifo.send_work(&work, work_id).expect("send work");
    }
}

async fn send_and_receive_test_workloads<'a>(
    work_sender: &'a mpsc::UnboundedSender<hal::MiningWork>,
    solution_receiver: &'a mut mpsc::UnboundedReceiver<hal::MiningWorkSolution>,
    n_send: usize,
    expected_solution_count: usize,
) {
    info!(
        "Sending {} work items and trying to receive {} solutions",
        n_send, expected_solution_count,
    );
    //
    // Put in some tasks
    for _ in 0..n_send {
        let work = prepare_test_work(1);
        work_sender.unbounded_send(work).expect("work send failed");
        // wait time to send out work + to compute work
        // TODO: come up with a formula instead of fixed time interval
        // wait = work_time * number_of_chips + time_to_send_out_a_jov
        await!(Delay::new(Instant::now() + Duration::from_millis(100))).unwrap();
    }
    let mut returned_solution_count = 0;
    loop {
        match await!(timeout_future(
            solution_receiver.next(),
            Duration::from_millis(1000)
        )) {
            TimeoutResult::TimedOut => break,
            TimeoutResult::Error => panic!("timeout error"),
            TimeoutResult::Returned(_solution) => returned_solution_count += 1,
        }
    }
    assert_eq!(
        returned_solution_count, expected_solution_count,
        "expected {} solutions but got {}",
        expected_solution_count, returned_solution_count
    );
}

fn start_hchain() -> HChainCtl {
    use super::power::VoltageCtrlBackend;

    let gpio_mgr = gpio::ControlPinManager::new();
    let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::new(0);
    let voltage_ctrl_backend =
        power::VoltageCtrlI2cSharedBlockingBackend::new(voltage_ctrl_backend);
    let midstate_count_log2 = MIDSTATE_CNT_A::ONE;

    let mut h_chain_ctl = super::HChainCtl::new(
        &gpio_mgr,
        voltage_ctrl_backend.clone(),
        config::S9_HASHBOARD_INDEX,
        midstate_count_log2,
        1,
    )
    .unwrap();
    h_chain_ctl.init().expect("h_chain init failed");
    h_chain_ctl
}

/// Verifies work generation for a hash chain
///
/// The test runs two batches of work:
/// - the first 3 work items are for initializing input queues of the chips and don't provide any
/// solutions
/// - the next 2 work items yield actual solutions. Since we don't push more work items, the
/// solution 1 never appears on the bus and leave chips output queues. This is fine as this test
/// is intended for initial check of correct operation
#[test]
fn test_work_generation() {
    // Create channels
    let (solution_sender, mut solution_receiver) = mpsc::unbounded();
    let (work_sender, work_receiver) = mpsc::unbounded();

    // Guard lives until the end of the block
    let _work_sender_guard = work_sender.clone();
    let _solution_sender_guard = solution_sender.clone();

    ii_async_compat::run_main_exits(async move {
        // Start HW
        let h_chain_ctl = Arc::new(Mutex::new(start_hchain()));

        // start HW receiver
        ii_async_compat::spawn(receiver_task(h_chain_ctl.clone(), solution_sender));

        // start HW sender
        ii_async_compat::spawn(sender_task(h_chain_ctl.clone(), work_receiver));

        // the first 3 work loads don't produce any solutions, these are merely to initialize the input
        // queue of each hashing chip
        await!(send_and_receive_test_workloads(
            &work_sender,
            &mut solution_receiver,
            3,
            0
        ));

        // submit 2 more work items, since we are intentionally being slow all chips should send a
        // solution for the submitted work
        let more_work_count = 2usize;
        let chip_count = await!(h_chain_ctl.lock())
            .expect("locking failed")
            .get_chip_count();
        let expected_solution_count = more_work_count * chip_count;

        await!(send_and_receive_test_workloads(
            &work_sender,
            &mut solution_receiver,
            more_work_count,
            expected_solution_count,
        ));
    });
}
