#![feature(await_macro, async_await)]

extern crate futures;
extern crate tokio;

use rminer::hal;
use rminer::hal::s9::gpio;
use rminer::hal::s9::power;
use rminer::hal::s9::registry;
use rminer::hal::HardwareCtl;
use rminer::misc::LOGGER;
use rminer::workhub;

use slog::{info, trace};

use std::time::{Duration, Instant, SystemTime};

use crate::hal::s9::fifo;
use futures_locks::Mutex;
use std::sync::Arc;
use tokio::await;
use tokio::prelude::*;
use tokio::timer::Delay;

/// Generates enough testing work until the work FIFO becomes full
/// The work is made unique by specifying a unique midstate.
///
/// As the next step the method starts collecting solutions, eliminating duplicates and extracting
/// valid solutions for further processing
///
/// Returns the amount of work generated during this run

async fn async_send_work<T>(
    work_registry: Arc<Mutex<registry::MiningWorkRegistry>>,
    h_chain_ctl: Arc<Mutex<hal::s9::HChainCtl<T>>>,
    mut tx_fifo: fifo::HChainFifo,
    mining_stats: Arc<Mutex<hal::MiningStats>>,
    workhub: workhub::WorkHub,
) where
    T: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    loop {
        await!(tx_fifo.async_wait_for_work_tx_room()).expect("wait for tx room");
        let test_work = await!(workhub.get_work());
        let work_id = await!(h_chain_ctl.lock())
            .expect("h_chain lock")
            .next_work_id();
        // send work is synchronous
        tx_fifo.send_work(&test_work, work_id).expect("send work");
        await!(work_registry.lock())
            .expect("locking ok")
            .store_work(work_id as usize, test_work);
        let mut stats = await!(mining_stats.lock()).expect("minig stats lock");
        stats.work_generated += 1;
        drop(stats);
    }
}

//solution_registry: Arc<Mutex<SolutionRegistry>>,
async fn async_recv_solutions<T>(
    work_registry: Arc<Mutex<registry::MiningWorkRegistry>>,
    mining_stats: Arc<Mutex<hal::MiningStats>>,
    h_chain_ctl: Arc<Mutex<hal::s9::HChainCtl<T>>>,
    mut rx_fifo: fifo::HChainFifo,
    workhub: workhub::WorkHub,
) where
    T: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    // solution receiving/filtering part
    loop {
        let solution = await!(rx_fifo.async_recv_solution())
            .expect("recv solution")
            .expect("solution is ok");
        let work_id = await!(h_chain_ctl.lock())
            .expect("h_chain lock")
            .get_work_id_from_solution_id(solution.solution_id) as usize;
        let mut stats = await!(mining_stats.lock()).expect("lock mining stats");
        let mut work_registry = await!(work_registry.lock()).expect("work registry lock failed");

        let work = work_registry.find_work(work_id);
        match work {
            Some(work_item) => {
                let solution_idx = await!(h_chain_ctl.lock())
                    .expect("h_chain lock")
                    .get_solution_idx_from_solution_id(solution.solution_id);
                let status = work_item.insert_solution(solution, solution_idx);

                // work item detected a new unique solution, we will push it for further processing
                if let Some(unique_solution) = status.unique_solution {
                    stats.unique_solutions += 1;
                    workhub.submit_solution(unique_solution);
                }
                stats.duplicate_solutions += status.duplicate as u64;
                stats.mismatched_solution_nonces += status.mismatched_nonce as u64;
            }
            None => {
                trace!(
                    LOGGER,
                    "No work present for solution, ID:{:#x} {:#010x?}",
                    work_id,
                    solution
                );
                stats.stale_solutions += 1;
            }
        }
    }
}

async fn async_hashrate_meter(mining_stats: Arc<Mutex<hal::MiningStats>>) {
    let hashing_started = SystemTime::now();
    let mut total_shares: u128 = 0;

    loop {
        await!(Delay::new(Instant::now() + Duration::from_secs(1))).unwrap();
        let mut stats = await!(mining_stats.lock()).expect("lock mining stats");
        {
            total_shares = total_shares + stats.unique_solutions as u128;
            // processing solution in the test simply means removing them
            stats.unique_solutions = 0;

            let total_hashing_time = hashing_started.elapsed().expect("time read ok");

            println!(
                "Hash rate: {} Gh/s",
                ((total_shares * (1u128 << 32)) as f32 / (total_hashing_time.as_secs() as f32))
                    * 1e-9_f32,
            );
            println!(
                "Total_shares: {}, total_time: {} s, total work generated: {}",
                total_shares,
                total_hashing_time.as_secs(),
                stats.work_generated,
            );
            println!(
                "Mismatched nonce count: {}, stale solutions: {}, duplicate solutions: {}",
                stats.mismatched_solution_nonces, stats.stale_solutions, stats.duplicate_solutions,
            );
        }
    }
}

fn start_hw(workhub: workhub::WorkHub, a_mining_stats: Arc<Mutex<hal::MiningStats>>) {
    use hal::s9::power::VoltageCtrlBackend;

    let gpio_mgr = gpio::ControlPinManager::new();
    let voltage_ctrl_backend = power::VoltageCtrlI2cBlockingBackend::new(0);
    let voltage_ctrl_backend =
        power::VoltageCtrlI2cSharedBlockingBackend::new(voltage_ctrl_backend);
    let mut h_chain_ctl = hal::s9::HChainCtl::new(
        &gpio_mgr,
        voltage_ctrl_backend.clone(),
        8,
        &s9_io::hchainio0::ctrl_reg::MIDSTATE_CNTW::ONE,
    )
    .unwrap();
    let work_registry = registry::MiningWorkRegistry::new();

    info!(LOGGER, "Initializing hash chain controller");
    h_chain_ctl.init().unwrap();
    info!(LOGGER, "Hash chain controller initialized");

    let a_work_registry = Arc::new(Mutex::new(work_registry));
    let a_h_chain_ctl = Arc::new(Mutex::new(h_chain_ctl));

    let c_h_chain_ctl = a_h_chain_ctl.clone();
    let c_work_registry = a_work_registry.clone();
    let c_mining_stats = a_mining_stats.clone();
    let c_workhub = workhub.clone();
    tokio::spawn_async(async move {
        let tx_fifo = await!(c_h_chain_ctl.lock()).unwrap().clone_fifo().unwrap();
        await!(async_send_work(
            c_work_registry,
            c_h_chain_ctl,
            tx_fifo,
            c_mining_stats,
            c_workhub,
        ));
    });
    let c_h_chain_ctl = a_h_chain_ctl.clone();
    let c_work_registry = a_work_registry.clone();
    let c_mining_stats = a_mining_stats.clone();
    let c_workhub = workhub.clone();
    tokio::spawn_async(async move {
        let rx_fifo = await!(c_h_chain_ctl.lock()).unwrap().clone_fifo().unwrap();
        await!(async_recv_solutions(
            c_work_registry,
            c_mining_stats,
            c_h_chain_ctl,
            rx_fifo,
            c_workhub,
        ));
    });
}

fn main() {
    tokio::run_async(async move {
        // Create workhub
        let (workhub, mut rx) = workhub::WorkHub::new();

        // Create mining stats
        let a_mining_stats = Arc::new(Mutex::new(hal::MiningStats::new()));

        // Start hardware
        let c_mining_stats = a_mining_stats.clone();
        start_hw(workhub.clone(), c_mining_stats);

        // Start hashrate-meter task
        let c_mining_stats = a_mining_stats.clone();
        tokio::spawn_async(async_hashrate_meter(c_mining_stats));

        // Receive solutions
        while let Some(_x) = await!(rx.next()) {}
    });
}
