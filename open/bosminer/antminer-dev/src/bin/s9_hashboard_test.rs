extern crate linux_embedded_hal;
extern crate rminer;
extern crate s9_io;
extern crate slog;
extern crate uint;

use rminer::hal;
use rminer::hal::s9::gpio;
use rminer::hal::s9::power;
use rminer::hal::HardwareCtl;
use rminer::misc::LOGGER;

use slog::{info, trace};

use linux_embedded_hal::I2cdev;

use std::thread;
use std::time::{Duration, SystemTime};

/// Maximum length of pending work list corresponds with the work ID range supported by the FPGA
const MAX_WORK_LIST_COUNT: usize = 65536;

/// Mining registry item contains work and solutions
#[derive(Clone, Debug)]
struct MiningWorkRegistryItem {
    work: hal::MiningWork,
    /// Each slot in the vector is associated with particular solution index as reported by
    /// the chips. Generally, hash board may fail to send a preceding solution due to
    /// corrupted communication frames. Therefore, each solution slot is optional.
    results: std::vec::Vec<Option<hal::MiningWorkResult>>,
}

impl MiningWorkRegistryItem {
    /// Associates a specified solution with mining work, accounts for duplicates and nonce
    /// mismatches
    /// * `solution` - solution to be inserted
    /// * `solution_idx` - each work may have multiple valid solutions, this index denotes its
    /// order. The index is reported by the hashing chip
    fn insert_solution(
        &mut self,
        solution: hal::MiningWorkResult,
        solution_idx: usize,
    ) -> InsertSolutionStatus {
        let mut status = InsertSolutionStatus {
            duplicate: false,
            mismatched_nonce: false,
            unique_solution: None,
        };
        // solution index determines the slot in solutions vector
        // if it's already present, we increment duplicate count
        if solution_idx < self.results.len() {
            if let Some(ref current_work_solution) = &self.results[solution_idx] {
                if current_work_solution.nonce != solution.nonce {
                    status.mismatched_nonce = true;
                } else {
                    status.duplicate = true;
                }
            }
        } else {
            // append empty slots so that we can process solutions that came out of order. This
            // is typically due to previously corrupted communication frames
            for _i in 0..solution_idx - self.results.len() {
                self.results.push(None);
            }

            self.results.push(Some(solution.clone()));

            let cloned_work = self.work.clone();

            // report the unique solution via status
            status.unique_solution = Some(UniqueMiningWorkSolution {
                timestamp: SystemTime::now(),
                work: cloned_work,
                solution: solution,
            });
        }
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

/// Container with mining work and a corresponding solution received at a particular time
/// This data structure is used when posting work+solution pairs for further submission upstream.
struct UniqueMiningWorkSolution {
    /// time stamp when it has been fetched from the result FIFO
    timestamp: std::time::SystemTime,
    /// Original mining work associated with this result
    work: hal::MiningWork,
    /// solution of the PoW puzzle
    solution: hal::MiningWorkResult,
}

/// Simple mining work registry that stores each work in a slot denoted by its work ID.
///
/// The slots are handled in circular fashion, when storing new work, any work older than
/// MAX_WORK_LIST_COUNT/2 sequence ID's in the past is to be retired.
struct MiningWorkRegistry {
    /// Current pending work list Each work item has a list of associated work results
    pending_work_list: std::vec::Vec<Option<MiningWorkRegistryItem>>,
    /// Keeps track of the ID, so that we can identify stale results
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
            results: std::vec::Vec::new(),
        });

        // retire old work that is not expected to have any result => work with ID older than
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
/// * `_i` - unique identifier that makes the work distinct
fn prepare_test_work(_i: u64) -> hal::MiningWork {
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

/// Generates enough testing work until the work FIFO becomes full
/// The work is made unique by specifying a unique midstate.
///
/// As the next step the method starts collecting results, eliminating duplicates and extracting
/// valid results for further processing
///
/// Returns the amount of work generated during this run
fn send_and_receive_test_workloads<T>(
    h_chain_ctl: &mut hal::s9::HChainCtl<T>,
    work_registry: &mut MiningWorkRegistry,
    solution_registry: &mut SolutionRegistry,
    midstate_start: &mut u64,
) -> usize
where
    T: 'static + Send + Sync + power::VoltageCtrlBackend,
{
    let mut work_generated = 0usize;
    // work sending part
    while !h_chain_ctl.is_work_tx_fifo_full() {
        let test_work = prepare_test_work(*midstate_start);
        let work_id = h_chain_ctl.send_work(&test_work).unwrap() as usize;
        work_registry.store_work(work_id, test_work);
        // the midstate identifier may wrap around (considering its size, effectively never...)
        *midstate_start = midstate_start.wrapping_add(1);
        work_generated += 1;
    }

    thread::sleep(Duration::from_millis(10));
    // result receiving/filtering part
    while let Some(solution) = h_chain_ctl.recv_work_result().unwrap() {
        let work_id = h_chain_ctl.get_work_id_from_result_id(solution.result_id) as usize;

        let mut work = work_registry.find_work(work_id);
        match work {
            Some(work_item) => {
                let solution_idx = h_chain_ctl.get_solution_idx_from_result_id(solution.result_id);
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
                    "No work present for result, ID:{:#x} {:#010x?}",
                    work_id,
                    solution
                );
                solution_registry.stale_solutions += 1;
            }
        }
    }
    work_generated
}

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
    let mut work_registry = MiningWorkRegistry::new();
    let mut solution_registry = SolutionRegistry::new();
    // sequence number when generating midstates
    let mut midstate_start = 0;
    let mut total_hashing_time: Duration = Duration::from_secs(0);
    let mut total_shares: u128 = 0;
    let mut total_work_generated: usize = 0;

    info!(LOGGER, "Initializing hash chain controller");
    h_chain_ctl.init().unwrap();
    info!(LOGGER, "Hash chain controller initialized");

    let mut last_hashrate_report = SystemTime::now();
    loop {
        total_work_generated += send_and_receive_test_workloads(
            &mut h_chain_ctl,
            &mut work_registry,
            &mut solution_registry,
            &mut midstate_start,
        );
        let last_hashrate_report_elapsed = last_hashrate_report.elapsed().unwrap();
        if last_hashrate_report_elapsed >= Duration::from_secs(1) {
            total_shares = total_shares + solution_registry.solutions.len() as u128;
            solution_registry.solutions.clear();

            total_hashing_time += last_hashrate_report_elapsed;
            println!(
                "Hash rate: {} Gh/s",
                ((total_shares * (1u128 << 32)) as f32 / (total_hashing_time.as_secs() as f32))
                    * 1e-9_f32,
            );
            println!(
                "Total_shares: {}, total_time: {} s, total work generated: {}",
                total_shares,
                total_hashing_time.as_secs(),
                total_work_generated,
            );
            println!(
                "Mismatched nonce count: {}, stale solutions: {}",
                solution_registry.mismatched_solution_nonces, solution_registry.stale_solutions
            );
            last_hashrate_report = SystemTime::now()
        }
    }
}

fn main() {
    test_work_generation();
}
