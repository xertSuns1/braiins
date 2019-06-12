#![feature(await_macro, async_await, futures_api)]

extern crate futures;
extern crate tokio;

use rminer::hal;
use rminer::hal::s9::gpio;
use rminer::hal::s9::power;
use rminer::hal::HardwareCtl;
use rminer::misc::LOGGER;

use slog::{info, trace};

use std::time::{Duration, Instant, SystemTime};

use crate::hal::s9::fifo;
//use futures::future::Future;
use futures_locks::Mutex;
use std::sync::Arc;
use tokio::await;
//use tokio::prelude::*;
use tokio::timer::Delay;

/// Maximum length of pending work list corresponds with the work ID range supported by the FPGA
const MAX_WORK_LIST_COUNT: usize = 65536;

/// Mining registry item contains work and solutions
#[derive(Clone, Debug)]
struct MiningWorkRegistryItem {
    work: hal::MiningWork,
    /// Each slot in the vector is associated with particular solution index as reported by
    /// the chips. Generally, hash board may fail to send a preceding solution due to
    /// corrupted communication frames. Therefore, each solution slot is optional.
    solutions: std::vec::Vec<hal::MiningWorkSolution>,
}

impl MiningWorkRegistryItem {
    /// Associates a specified solution with mining work, accounts for duplicates and nonce
    /// mismatches
    /// * `solution` - solution to be inserted
    /// * `solution_idx` - each work may have multiple valid solutions, this index denotes its
    /// order. The index is reported by the hashing chip
    fn insert_solution(
        &mut self,
        new_solution: hal::MiningWorkSolution,
        _solution_idx: usize,
    ) -> InsertSolutionStatus {
        let mut status = InsertSolutionStatus {
            duplicate: false,
            mismatched_nonce: false,
            unique_solution: None,
        };
        // scan the current solutions and detect a duplicate
        for solution in self.solutions.iter() {
            if solution.nonce == new_solution.nonce {
                status.duplicate = true;
                return status;
            }
        }

        // At this point, we know such solution has not been received yet. If it is valid (no
        // hardware error detected == meets the target), it can be appended to the solution list
        // for this work item
        // TODO: call the evaluator for the solution
        self.solutions.push(new_solution.clone());

        let cloned_work = self.work.clone();
        // report the unique solution via status
        status.unique_solution = Some(UniqueMiningWorkSolution {
            timestamp: SystemTime::now(),
            work: cloned_work,
            solution: new_solution,
        });
        status
    }
}

/// Helper container for the status after inserting the solution
struct InsertSolutionStatus {
    /// Nonce of the solution at a given index doesn't match the existing nonce
    mismatched_nonce: bool,
    /// Solution is duplicate (given MiningWorkRegistryItem) already has it
    duplicate: bool,
    /// actual solution (defined if the above 2 are false)
    unique_solution: Option<UniqueMiningWorkSolution>,
}

#[allow(dead_code)]
/// Container with mining work and a corresponding solution received at a particular time
/// This data structure is used when posting work+solution pairs for further submission upstream.
struct UniqueMiningWorkSolution {
    /// time stamp when it has been fetched from the solution FIFO
    timestamp: std::time::SystemTime,
    /// Original mining work associated with this solution
    work: hal::MiningWork,
    /// solution of the PoW puzzle
    solution: hal::MiningWorkSolution,
}

/// Simple mining work registry that stores each work in a slot denoted by its work ID.
///
/// The slots are handled in circular fashion, when storing new work, any work older than
/// MAX_WORK_LIST_COUNT/2 sequence ID's in the past is to be retired.
struct MiningWorkRegistry {
    /// Current pending work list Each work item has a list of associated work solutions
    pending_work_list: std::vec::Vec<Option<MiningWorkRegistryItem>>,
    /// Keeps track of the ID, so that we can identify stale solutions
    last_work_id: Option<usize>,
}

impl MiningWorkRegistry {
    fn new() -> Self {
        Self {
            pending_work_list: vec![None; MAX_WORK_LIST_COUNT],
            last_work_id: None,
        }
    }

    /// Helper method that performs modulo subtraction on the indices of the vector.
    /// This enables circular buffer arithmetic
    #[inline]
    fn index_sub(x: usize, y: usize) -> usize {
        x.wrapping_sub(y).wrapping_add(MAX_WORK_LIST_COUNT) % MAX_WORK_LIST_COUNT
    }

    /// Stores new work in the registry and retires (removes) any stale work with ID
    /// older than 1/2 of MAX_WORK_LIST_COUNT
    /// * `id` - identifies the work
    /// * `work` - new work to be stored
    fn store_work(&mut self, id: usize, work: hal::MiningWork) {
        // The slot must be empty
        assert!(
            self.pending_work_list[id].is_none(),
            "Slot at index {} is not empty",
            id
        );
        // and the new work has to be sequenced
        if let Some(last_work_id) = self.last_work_id {
            assert_eq!(
                Self::index_sub(id, last_work_id),
                1,
                "Work id is out of sequence {}",
                id
            )
        }

        self.last_work_id = Some(id);

        self.pending_work_list[id] = Some(MiningWorkRegistryItem {
            work,
            solutions: std::vec::Vec::new(),
        });

        // retire old work that is not expected to have any solution => work with ID older than
        // MAX_WORK_LIST_COUNT/2 is marked obsolete
        let retire_id = Self::index_sub(id, MAX_WORK_LIST_COUNT / 2);

        self.pending_work_list[retire_id] = None;
    }

    fn find_work(&mut self, id: usize) -> &mut Option<MiningWorkRegistryItem> {
        &mut self.pending_work_list[id]
    }
}

/// A registry of solutions with small statistics
struct SolutionRegistry {
    /// Unique solutions
    solutions: std::vec::Vec<UniqueMiningWorkSolution>,
    /// Number of stale solutions received from the hardware
    stale_solutions: u64,
    /// Unable to feed the hardware fast enough results in duplicate solutions as
    /// multiple chips may process the same mining work
    duplicate_solutions: u64,
    /// Keep track of nonces that didn't match with previously received solutions (after
    /// filtering hardware errors, this should really stay at 0, otherwise we have some weird
    /// hardware problem)
    mismatched_solution_nonces: u64,
}

impl SolutionRegistry {
    fn new() -> Self {
        Self {
            solutions: std::vec::Vec::new(),
            stale_solutions: 0,
            duplicate_solutions: 0,
            mismatched_solution_nonces: 0,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_store_work_start() {
        let mut registry = MiningWorkRegistry::new();
        let work = prepare_test_work(0);

        registry.store_work(0, work);
    }

    #[test]
    #[should_panic]
    fn test_store_work_out_of_sequence_work_id() {
        let mut registry = MiningWorkRegistry::new();
        let work1 = prepare_test_work(0);
        let work2 = prepare_test_work(1);
        // store initial work
        registry.store_work(0, work1);
        // this should trigger a panic
        registry.store_work(2, work2);
    }

    #[test]
    fn test_store_work_retiring() {
        let mut registry = MiningWorkRegistry::new();
        // after exhausting the full work list count, the first half of the slots must be retired
        for id in 0..MAX_WORK_LIST_COUNT {
            let work = prepare_test_work(id as u64);
            registry.store_work(id, work);
        }
        // verify the first half being empty
        for id in 0..MAX_WORK_LIST_COUNT / 2 {
            assert!(
                registry.pending_work_list[0].is_none(),
                "Work at id {} was expected to be retired!",
                id
            );
        }
        // verify the second half being non-empty
        for id in MAX_WORK_LIST_COUNT / 2..MAX_WORK_LIST_COUNT {
            assert!(
                registry.pending_work_list[id].is_some(),
                "Work at id {} was expected to be defined!",
                id
            );
        }

        // store one more item should retire work at index MAX_WORK_LIST_COUNT/2
        let retire_idx_half = MAX_WORK_LIST_COUNT / 2;
        registry.store_work(0, prepare_test_work(0));
        assert!(
            registry.pending_work_list[retire_idx_half].is_none(),
            "Work at {} was expected to be retired (after overwriting idx 0)",
            retire_idx_half
        );
    }
}

/// * `i` - unique identifier for the generated midstate
fn prepare_test_work(i: u64) -> hal::MiningWork {
    hal::MiningWork {
        version: 0,
        extranonce_2: 0,
        midstates: vec![uint::U256([i, 0, 0, 0])],
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

/// Generates enough testing work until the work FIFO becomes full
/// The work is made unique by specifying a unique midstate.
///
/// As the next step the method starts collecting solutions, eliminating duplicates and extracting
/// valid solutions for further processing
///
/// Returns the amount of work generated during this run

async fn async_send_work<T>(
    work_registry: Arc<Mutex<MiningWorkRegistry>>,
    h_chain_ctl: Arc<Mutex<hal::s9::HChainCtl<T>>>,
    mut tx_fifo: fifo::HChainFifo,
    work_generated: Arc<Mutex<usize>>,
) where
    T: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    let mut midstate_start = 0;
    loop {
        await!(tx_fifo.async_wait_for_work_tx_room()).expect("wait for tx room");

        let test_work = prepare_test_work(midstate_start);
        let work_id = await!(h_chain_ctl.lock())
            .expect("h_chain lock")
            .next_work_id();
        // send work is synchronous
        tx_fifo.send_work(&test_work, work_id).expect("send work");
        await!(work_registry.lock())
            .expect("locking ok")
            .store_work(work_id as usize, test_work);
        // the midstate identifier may wrap around (considering its size, effectively never...)
        midstate_start = midstate_start.wrapping_add(1);
        *await!(work_generated.lock()).expect("lock counter") += 1;
    }
}

async fn async_recv_solutions<T>(
    work_registry: Arc<Mutex<MiningWorkRegistry>>,
    solution_registry: Arc<Mutex<SolutionRegistry>>,
    h_chain_ctl: Arc<Mutex<hal::s9::HChainCtl<T>>>,
    mut rx_fifo: fifo::HChainFifo,
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

        let mut work_registry = await!(work_registry.lock()).expect("locking ok");
        let mut solution_registry =
            await!(solution_registry.lock()).expect("solution registry lock");
        let work = work_registry.find_work(work_id);
        match work {
            Some(work_item) => {
                let solution_idx = await!(h_chain_ctl.lock())
                    .expect("h_chain lock")
                    .get_solution_idx_from_solution_id(solution.solution_id);
                let status = work_item.insert_solution(solution, solution_idx);

                // work item detected a new unique solution, we will push it for further processing
                if let Some(unique_solution) = status.unique_solution {
                    solution_registry.solutions.push(unique_solution);
                }
                solution_registry.duplicate_solutions += status.duplicate as u64;
                solution_registry.mismatched_solution_nonces += status.mismatched_nonce as u64;
            }
            None => {
                trace!(
                    LOGGER,
                    "No work present for solution, ID:{:#x} {:#010x?}",
                    work_id,
                    solution
                );
                solution_registry.stale_solutions += 1;
            }
        }
    }
}

async fn async_hashrate_meter(
    solution_registry: Arc<Mutex<SolutionRegistry>>,
    work_generated: Arc<Mutex<usize>>,
) {
    let hashing_started = SystemTime::now();
    let mut total_shares: u128 = 0;

    loop {
        await!(Delay::new(Instant::now() + Duration::from_secs(1))).unwrap();

        let total_work_generated = await!(work_generated.lock()).expect("lock counter");
        {
            let mut solution_registry =
                await!(solution_registry.lock()).expect("solution registry lock");

            total_shares = total_shares + solution_registry.solutions.len() as u128;
            // processing solution in the test simply means removing them
            solution_registry.solutions.clear();

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
                *total_work_generated,
            );
            println!(
                "Mismatched nonce count: {}, stale solutions: {}, duplicate solutions: {}",
                solution_registry.mismatched_solution_nonces,
                solution_registry.stale_solutions,
                solution_registry.duplicate_solutions,
            );
        }
    }
}

fn test_work_generation() {
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
    let work_registry = MiningWorkRegistry::new();
    let solution_registry = SolutionRegistry::new();

    info!(LOGGER, "Initializing hash chain controller");
    h_chain_ctl.init().unwrap();
    info!(LOGGER, "Hash chain controller initialized");

    let work_generated = 0usize;

    let a_work_registry = Arc::new(Mutex::new(work_registry));
    let a_solution_registry = Arc::new(Mutex::new(solution_registry));
    let a_work_generated = Arc::new(Mutex::new(work_generated));
    let a_h_chain_ctl = Arc::new(Mutex::new(h_chain_ctl));

    tokio::run_async(async move {
        let c_h_chain_ctl = a_h_chain_ctl.clone();
        let c_work_registry = a_work_registry.clone();
        let c_work_generated = a_work_generated.clone();
        tokio::spawn_async(async move {
            let tx_fifo = await!(c_h_chain_ctl.lock()).unwrap().clone_fifo().unwrap();
            await!(async_send_work(
                c_work_registry,
                c_h_chain_ctl,
                tx_fifo,
                c_work_generated,
            ));
        });
        let c_h_chain_ctl = a_h_chain_ctl.clone();
        let c_work_registry = a_work_registry.clone();
        let c_solution_registry = a_solution_registry.clone();
        tokio::spawn_async(async move {
            let rx_fifo = await!(c_h_chain_ctl.lock()).unwrap().clone_fifo().unwrap();
            await!(async_recv_solutions(
                c_work_registry,
                c_solution_registry,
                c_h_chain_ctl,
                rx_fifo,
            ));
        });
        let c_solution_registry = a_solution_registry.clone();
        let c_work_generated = a_work_generated.clone();
        await!(async_hashrate_meter(c_solution_registry, c_work_generated,));
    });
}

fn main() {
    test_work_generation();
}
