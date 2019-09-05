// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

use ii_logging::macros::*;

use crate::hal;

use tokio::timer::Delay;

use futures::compat::Future01CompatExt;
use futures::lock::Mutex;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Share=1 represents a space of 2^32 calculated hashes for Bitcoin
/// mainnet (exactly 2^256/(0xffff<<208), where 0xffff<<208 is defined
/// as target @ difficulty 1 for Bitcoin mainet).
/// TODO: This algorithm needs be adjusted for other coins/test environments in the future
/// Shares at dificulty X takes X times more hashes to compute.
fn shares_to_giga_hashes(shares: u128) -> f64 {
    (shares << 32) as f64 * 1e-9
}

pub async fn hashrate_meter_task_hashchain(mining_stats: Arc<Mutex<hal::MiningStats>>) {
    let mut last_stat_time = Instant::now();
    let mut old_error_stats = Default::default();
    loop {
        await!(Delay::new(Instant::now() + Duration::from_secs(1)).compat())
            .expect("stats delay wait failed");

        let mut stats = await!(mining_stats.lock());
        let solved_shares = stats.unique_solutions_shares;
        stats.unique_solutions_shares = 0;
        let work_generated = stats.work_generated;
        stats.work_generated = 0;
        let unique_solutions = stats.unique_solutions;
        stats.unique_solutions = 0;

        let hashing_time = last_stat_time.elapsed().as_secs_f64();

        info!(
            "Hashchain hash rate: generated {:.2} Gh/s, computed {:.2} Gh/s",
            shares_to_giga_hashes(work_generated as u128) / hashing_time,
            shares_to_giga_hashes(solved_shares as u128) / hashing_time,
        );

        if work_generated == 0 {
            trace!("No work is being generated!");
        }
        if unique_solutions == 0 {
            trace!("No work is being solved!");
        }

        if stats.error_stats != old_error_stats {
            let error_stats = stats.error_stats.clone();
            info!(

                "Mismatched nonce count: {}, stale solutions: {}, duplicate solutions: {}, hardware errors: {}",
                error_stats.mismatched_solution_nonces,
                error_stats.stale_solutions,
                error_stats.duplicate_solutions,
                error_stats.hardware_errors,
            );
            old_error_stats = error_stats;
        }

        last_stat_time = Instant::now();
    }
}

static SUBMITTED_SHARE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn account_solution(target: &ii_bitcoin::Target) {
    let difficulty = target.get_difficulty() as u64;
    SUBMITTED_SHARE_COUNTER.fetch_add(difficulty, Ordering::SeqCst);
}

pub async fn hashrate_meter_task() {
    let hashing_started = Instant::now();
    let mut total_shares: u128 = 0;

    loop {
        await!(Delay::new(Instant::now() + Duration::from_secs(1)).compat())
            .expect("stats delay wait failed");
        total_shares += SUBMITTED_SHARE_COUNTER.swap(0, Ordering::SeqCst) as u128;
        let total_hashing_time = hashing_started.elapsed();
        info!(
            "Hash rate from submitted shares: {:.2} Gh/s",
            shares_to_giga_hashes(total_shares) / total_hashing_time.as_secs_f64(),
        );
    }
}
