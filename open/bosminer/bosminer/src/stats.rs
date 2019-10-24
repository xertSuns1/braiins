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

use crate::node;

use ii_stats::WindowedTimeMean;

use futures::lock::{Mutex, MutexGuard};
use ii_async_compat::{futures, tokio};
use tokio::timer::delay_for;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time;

use lazy_static::lazy_static;

lazy_static! {
    static ref DEFAULT_TIME_MEAN_INTERVALS: Vec<time::Duration> = vec![
        time::Duration::from_secs(5),
        time::Duration::from_secs(1 * 60),
        time::Duration::from_secs(5 * 60),
        time::Duration::from_secs(15 * 60),
        time::Duration::from_secs(24 * 60 * 60),
    ];
}

struct MeterInner {
    /// All shares measured from the beginning of mining
    shares: ii_bitcoin::Shares,
    /// Approximate arithmetic mean of hashes within given time intervals (in kH/time)
    time_means: Vec<WindowedTimeMean>,
}

#[derive(Debug)]
pub struct Meter {
    inner: Mutex<MeterInner>,
}

impl Meter {
    pub fn new(intervals: &Vec<time::Duration>) -> Self {
        Self {
            inner: Mutex::new(MeterInner {
                shares: Default::default(),
                time_means: intervals
                    .iter()
                    .map(|&interval| WindowedTimeMean::new(interval))
                    .collect(),
            }),
        }
    }

    pub async fn shares(&self) -> SharesGuard<'_> {
        SharesGuard(self.inner.lock().await)
    }

    pub async fn time_means(&self) -> TimeMeansGuard<'_> {
        TimeMeansGuard(self.inner.lock().await)
    }

    pub(crate) async fn account_solution(&self, target: &ii_bitcoin::Target, time: time::Instant) {
        let mut meter = self.inner.lock().await;
        let kilo_hashes = ii_bitcoin::Shares::new(target).into_kilo_hashes();

        meter.shares.account_solution(target);
        for time_mean in &mut meter.time_means {
            time_mean.insert(kilo_hashes, time);
        }
    }
}

impl Default for Meter {
    fn default() -> Self {
        Self::new(DEFAULT_TIME_MEAN_INTERVALS.as_ref())
    }
}

pub struct SharesGuard<'a>(MutexGuard<'a, MeterInner>);

impl<'a> std::ops::Deref for SharesGuard<'a> {
    type Target = ii_bitcoin::Shares;

    fn deref(&self) -> &Self::Target {
        &self.0.shares
    }
}

pub struct TimeMeansGuard<'a>(MutexGuard<'a, MeterInner>);

impl<'a> std::ops::Deref for TimeMeansGuard<'a> {
    type Target = Vec<WindowedTimeMean>;

    fn deref(&self) -> &Self::Target {
        &self.0.time_means
    }
}

#[derive(Debug)]
pub struct Mining {
    pub start_time: time::Instant,
    pub accepted: Meter,
    pub rejected: Meter,
    pub backend_error: Meter,
}

impl Mining {
    pub fn new(start_time: time::Instant, intervals: &Vec<time::Duration>) -> Self {
        Self {
            start_time,
            accepted: Meter::new(&intervals),
            rejected: Meter::new(&intervals),
            backend_error: Meter::new(&intervals),
        }
    }
}

impl Default for Mining {
    fn default() -> Self {
        Self::new(time::Instant::now(), DEFAULT_TIME_MEAN_INTERVALS.as_ref())
    }
}

pub(crate) async fn account_accepted(
    path: &node::Path,
    solution_target: &ii_bitcoin::Target,
    time: time::Instant,
) {
    for node in path {
        node.mining_stats()
            .accepted
            .account_solution(solution_target, time)
            .await;
    }
}

/// Holds all error statistics
#[derive(Clone, PartialEq, Eq, Default)]
pub struct MiningError {
    /// Number of submitted results that are not hitting ASIC target
    pub hardware_errors: u64,
    /// Number of stale solutions received from the hardware
    pub stale_solutions: u64,
    /// Unable to feed the hardware fast enough results in duplicate solutions as
    /// multiple chips may process the same mining work
    pub duplicate_solutions: u64,
    /// Keep track of nonces that didn't match with previously received solutions (after
    /// filtering hardware errors, this should really stay at 0, otherwise we have some weird
    /// hardware problem)
    pub mismatched_solution_nonces: u64,
}

/// Holds all hardware-related statistics for a hashchain
#[derive(Clone, PartialEq, Eq, Default)]
pub struct MiningObsolete {
    /// Number of work items generated for the hardware
    pub work_generated: usize,
    /// Counter of unique solutions
    pub unique_solutions: u64,
    /// Amount of computed work in shares (for example one work computed at difficulty 64 is 64 shares)
    pub unique_solutions_shares: u64,
    /// Error statistics
    pub error_stats: MiningError,
}

impl MiningObsolete {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }
}

/// Share=1 represents a space of 2^32 calculated hashes for Bitcoin
/// mainnet (exactly 2^256/(0xffff<<208), where 0xffff<<208 is defined
/// as target @ difficulty 1 for Bitcoin mainet).
/// TODO: This algorithm needs be adjusted for other coins/test environments in the future
/// Shares at dificulty X takes X times more hashes to compute.
fn shares_to_giga_hashes(shares: u128) -> f64 {
    (shares << 32) as f64 * 1e-9
}

pub async fn hashrate_meter_task_hashchain(mining_stats: Arc<Mutex<MiningObsolete>>) {
    let mut last_stat_time = time::Instant::now();
    let mut old_error_stats = Default::default();
    loop {
        delay_for(time::Duration::from_secs(1)).await;

        let mut stats = mining_stats.lock().await;
        let solved_shares = stats.unique_solutions_shares;
        stats.unique_solutions_shares = 0;
        let work_generated = stats.work_generated;
        stats.work_generated = 0;
        let unique_solutions = stats.unique_solutions;
        stats.unique_solutions = 0;

        let hashing_time = last_stat_time.elapsed().as_secs_f64();

        if solved_shares > 0 {
            info!(
                "Hash rate @ ASIC difficulty: {:.2} Gh/s",
                shares_to_giga_hashes(solved_shares as u128) / hashing_time,
            );
        }
        if work_generated == 0 {
            trace!("No work is being generated!");
        } else {
            trace!(
                "Hash rate of generated work: {:.2} Gh/s",
                shares_to_giga_hashes(work_generated as u128) / hashing_time,
            );
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

        last_stat_time = time::Instant::now();
    }
}

static SUBMITTED_SHARE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn account_solution(target: &ii_bitcoin::Target) {
    let difficulty = target.get_difficulty() as u64;
    SUBMITTED_SHARE_COUNTER.fetch_add(difficulty, Ordering::SeqCst);
}

pub async fn hashrate_meter_task() {
    let hashing_started = time::Instant::now();
    let mut total_shares: u128 = 0;

    loop {
        delay_for(time::Duration::from_secs(1)).await;

        total_shares += SUBMITTED_SHARE_COUNTER.swap(0, Ordering::SeqCst) as u128;
        let total_hashing_time = hashing_started.elapsed();
        if total_shares > 0 {
            info!(
                "Hash rate @ pool difficulty: {:.2} Gh/s",
                shares_to_giga_hashes(total_shares) / total_hashing_time.as_secs_f64(),
            );
        }
    }
}
