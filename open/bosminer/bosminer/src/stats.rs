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
use crate::stats;
use crate::work;

use bosminer_macros::{ClientStats, MiningStats, WorkSolverStats};

use ii_stats::WindowedTimeMean;

use futures::lock::Mutex;
use ii_async_compat::{futures, tokio};
use tokio::time::delay_for;

use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time;

use once_cell::sync::Lazy;

pub static TIME_MEAN_INTERVAL_5S: Lazy<time::Duration> = Lazy::new(|| time::Duration::from_secs(5));
pub static TIME_MEAN_INTERVAL_1M: Lazy<time::Duration> =
    Lazy::new(|| time::Duration::from_secs(1 * 60));
pub static TIME_MEAN_INTERVAL_5M: Lazy<time::Duration> =
    Lazy::new(|| time::Duration::from_secs(5 * 60));
pub static TIME_MEAN_INTERVAL_15M: Lazy<time::Duration> =
    Lazy::new(|| time::Duration::from_secs(15 * 60));
pub static TIME_MEAN_INTERVAL_24H: Lazy<time::Duration> =
    Lazy::new(|| time::Duration::from_secs(24 * 60 * 60));

static DEFAULT_TIME_MEAN_INTERVALS: Lazy<Vec<time::Duration>> = Lazy::new(|| {
    vec![
        *TIME_MEAN_INTERVAL_5S,
        *TIME_MEAN_INTERVAL_1M,
        *TIME_MEAN_INTERVAL_5M,
        *TIME_MEAN_INTERVAL_15M,
        *TIME_MEAN_INTERVAL_24H,
    ]
});

/// Auxiliary structure for adding time to snapshots
pub struct Snapshot<T> {
    pub snapshot_time: time::Instant,
    inner: T,
}

impl<T> Snapshot<T> {
    pub fn new(inner: T) -> Self {
        Self {
            snapshot_time: time::Instant::now(),
            inner,
        }
    }
}

impl<T> std::ops::Deref for Snapshot<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Represents a snapshot of all statistics at an instant
#[derive(Debug, Clone)]
pub struct MeterSnapshot {
    /// Number of solutions measured from the beginning of the mining
    pub solutions: u64,
    /// All shares measured from the beginning of the mining
    pub shares: ii_bitcoin::Shares,
    /// Approximate arithmetic mean of hashes within given time intervals (in kH/time)
    time_means: Vec<WindowedTimeMean>,
}

impl MeterSnapshot {
    fn get_time_mean(&self, interval: time::Duration) -> &WindowedTimeMean {
        self.time_means
            .iter()
            .find(|time_mean| time_mean.interval() == interval)
            .expect("cannot find given time interval")
    }

    #[inline]
    pub fn to_kilo_hashes(
        &self,
        interval: time::Duration,
        now: time::Instant,
    ) -> ii_bitcoin::HashesUnit {
        ii_bitcoin::HashesUnit::KiloHashes(self.get_time_mean(interval).measure(now))
    }

    #[inline]
    pub fn to_mega_hashes(
        &self,
        interval: time::Duration,
        now: time::Instant,
    ) -> ii_bitcoin::HashesUnit {
        self.to_kilo_hashes(interval, now).into_mega_hashes()
    }

    #[inline]
    pub fn to_giga_hashes(
        &self,
        interval: time::Duration,
        now: time::Instant,
    ) -> ii_bitcoin::HashesUnit {
        self.to_kilo_hashes(interval, now).into_giga_hashes()
    }

    #[inline]
    pub fn to_tera_hashes(
        &self,
        interval: time::Duration,
        now: time::Instant,
    ) -> ii_bitcoin::HashesUnit {
        self.to_kilo_hashes(interval, now).into_tera_hashes()
    }

    #[inline]
    pub fn to_pretty_hashes(
        &self,
        interval: time::Duration,
        now: time::Instant,
    ) -> ii_bitcoin::HashesUnit {
        self.to_kilo_hashes(interval, now).into_pretty_hashes()
    }
}

#[derive(Debug)]
pub struct Meter {
    inner: Mutex<MeterSnapshot>,
}

impl Meter {
    pub fn new(intervals: &Vec<time::Duration>) -> Self {
        Self {
            inner: Mutex::new(MeterSnapshot {
                solutions: 0,
                shares: Default::default(),
                time_means: intervals
                    .iter()
                    .map(|&interval| WindowedTimeMean::new(interval))
                    .collect(),
            }),
        }
    }

    pub async fn take_snapshot(&self) -> Snapshot<MeterSnapshot> {
        Snapshot::new(self.inner.lock().await.clone())
    }

    pub(crate) async fn account_solution(&self, target: &ii_bitcoin::Target, time: time::Instant) {
        let mut meter = self.inner.lock().await;
        let kilo_hashes = ii_bitcoin::Shares::new(target)
            .into_kilo_hashes()
            .into_f64();

        // TODO: what to do when number overflows
        meter.solutions += 1;
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

#[derive(Debug, Clone)]
pub struct LastShareSnapshot {
    /// Time when the last share has been submitted
    pub time: time::SystemTime,
    /// Difficulty of the last share
    pub difficulty: usize,
}

#[derive(Debug)]
pub struct LastShare {
    inner: Mutex<Option<LastShareSnapshot>>,
}

impl LastShare {
    pub async fn take_snapshot(&self) -> Option<Snapshot<LastShareSnapshot>> {
        self.inner
            .lock()
            .await
            .clone()
            .map(|inner| Snapshot::new(inner))
    }

    pub(crate) async fn account_solution(
        &self,
        target: &ii_bitcoin::Target,
        time: time::SystemTime,
    ) {
        self.inner.lock().await.replace(LastShareSnapshot {
            time,
            difficulty: target.get_difficulty(),
        });
    }
}

impl Default for LastShare {
    fn default() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

#[derive(Debug)]
pub struct BestShare {
    inner: AtomicUsize,
}

impl BestShare {
    const INVALID_DIFFICULTY: usize = 0;
    pub fn take_snapshot(&self) -> Option<Snapshot<usize>> {
        let difficulty = self.inner.load(Ordering::Relaxed);
        if difficulty == Self::INVALID_DIFFICULTY {
            None
        } else {
            Some(Snapshot::new(difficulty))
        }
    }

    pub(crate) fn account_solution(&self, target: &ii_bitcoin::Target) {
        let new_diff = target.get_difficulty();
        let mut old_diff = self.inner.load(Ordering::Relaxed);

        while old_diff < new_diff {
            let prev_diff = self
                .inner
                .compare_and_swap(old_diff, new_diff, Ordering::Relaxed);
            if old_diff == prev_diff {
                break;
            } else {
                old_diff = prev_diff;
            }
        }
    }
}

impl Default for BestShare {
    fn default() -> Self {
        Self {
            inner: AtomicUsize::new(Self::INVALID_DIFFICULTY),
        }
    }
}

pub trait AtomicCounter: Debug {
    /// The underlying type
    type Type: Default;

    /// Create new instance of atomic counter initialized to given value
    fn new(value: Self::Type) -> Self;
    /// Increment the current value
    fn inc(&self);
    /// Adds to the current value
    fn add(&self, value: Self::Type);
    /// Loads a value from the atomic type
    fn load(&self) -> Self::Type;
}

macro_rules! atomic_counter_impl (
    ($atomic_type:path, $base_type:path) => (
        impl AtomicCounter for $atomic_type {
            type Type = $base_type;

            #[inline]
            fn new(value: Self::Type) -> Self {
                Self::new(value)
            }

            #[inline]
            fn inc(&self) {
                self.add(1);
            }

            #[inline]
            fn add(&self, value: Self::Type) {
                self.fetch_add(value, Ordering::Relaxed);
            }

            #[inline]
            fn load(&self) -> Self::Type {
                self.load(Ordering::Relaxed)
            }
        }
    )
);

/// An atomic counter that supports timestamped snapshotting
#[derive(Debug)]
pub struct Counter<T> {
    inner: T,
}

impl<T> Counter<T>
where
    T: AtomicCounter,
{
    pub fn new(value: T::Type) -> Self {
        Self {
            inner: T::new(value),
        }
    }

    pub fn take_snapshot(&self) -> Snapshot<T::Type> {
        Snapshot::new(self.inner.load())
    }

    #[inline]
    pub fn inc(&self) {
        self.inner.inc();
    }

    #[inline]
    pub fn add(&self, count: T::Type) {
        self.inner.add(count);
    }
}

impl<T> Default for Counter<T>
where
    T: AtomicCounter,
{
    fn default() -> Self {
        Self::new(Default::default())
    }
}

atomic_counter_impl!(AtomicU64, u64);
atomic_counter_impl!(AtomicUsize, usize);

pub type CounterU64 = Counter<AtomicU64>;
pub type CounterUsize = Counter<AtomicUsize>;

#[derive(Debug)]
pub struct Timestamp {
    inner: Mutex<Option<time::SystemTime>>,
}

impl Timestamp {
    pub fn new<T: Into<Option<time::SystemTime>>>(time: T) -> Self {
        Self {
            inner: Mutex::new(time.into()),
        }
    }

    pub async fn take_snapshot(&self) -> Option<Snapshot<time::SystemTime>> {
        self.inner.lock().await.map(|inner| Snapshot::new(inner))
    }

    pub async fn touch<T: Into<Option<time::SystemTime>>>(&self, time: T) {
        self.inner
            .lock()
            .await
            .replace(time.into().unwrap_or_else(|| time::SystemTime::now()));
    }
}

impl Default for Timestamp {
    fn default() -> Self {
        Self::new(None)
    }
}

pub trait UnixTime {
    fn get_unix_time(&self) -> Result<u32, String>;
}

impl UnixTime for time::SystemTime {
    fn get_unix_time(&self) -> Result<u32, String> {
        self.duration_since(time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as u32)
            .map_err(|e| format!("{}", e))
    }
}

pub trait Mining: Send + Sync {
    /// The time all statistics are measured from
    fn start_time(&self) -> &time::Instant;
    /// Information about last valid share with at least job difficulty
    fn last_share(&self) -> &LastShare;
    fn best_share(&self) -> &BestShare;
    /// Statistics for all valid blocks on network difficulty
    fn valid_network_diff(&self) -> &Meter;
    /// Statistics for all valid jobs on job/pool difficulty
    fn valid_job_diff(&self) -> &Meter;
    /// Statistics for all valid work on backend difficulty
    fn valid_backend_diff(&self) -> &Meter;
    /// Statistics for all invalid work on backend difficulty (backend/HW error)
    fn error_backend_diff(&self) -> &Meter;
}

pub trait Client: Mining {
    /// Number of valid jobs received from remote server
    fn valid_jobs(&self) -> &CounterUsize;
    /// Number of invalid jobs received from remote server
    fn invalid_jobs(&self) -> &CounterUsize;
    /// Number of work generated from jobs by rolling or with extra nonce
    fn generated_work(&self) -> &CounterU64;
    /// Shares accepted by remote server
    fn accepted(&self) -> &Meter;
    /// Shares rejected by remote server
    fn rejected(&self) -> &Meter;
    /// Valid shares rejected by remote server or discarded due to some error
    fn stale(&self) -> &Meter;
}

pub trait WorkSolver: Mining {
    /// The time when the device get last work for solution
    fn last_work_time(&self) -> &Timestamp;
    /// Number of work generated from jobs by rolling or with extra nonce
    fn generated_work(&self) -> &CounterU64;
}

#[derive(Debug, MiningStats)]
pub struct BasicMining {
    #[member_start_time]
    pub start_time: time::Instant,
    #[member_last_share]
    pub last_share: LastShare,
    #[member_best_share]
    pub best_share: BestShare,
    #[member_valid_network_diff]
    pub valid_network_diff: Meter,
    #[member_valid_job_diff]
    pub valid_job_diff: Meter,
    #[member_valid_backend_diff]
    pub valid_backend_diff: Meter,
    #[member_error_backend_diff]
    pub error_backend_diff: Meter,
}

impl BasicMining {
    pub fn new(start_time: time::Instant, intervals: &Vec<time::Duration>) -> Self {
        Self {
            start_time,
            last_share: Default::default(),
            best_share: Default::default(),
            valid_network_diff: Meter::new(&intervals),
            valid_job_diff: Meter::new(&intervals),
            valid_backend_diff: Meter::new(&intervals),
            error_backend_diff: Meter::new(&intervals),
        }
    }
}

impl Default for BasicMining {
    fn default() -> Self {
        Self::new(time::Instant::now(), DEFAULT_TIME_MEAN_INTERVALS.as_ref())
    }
}

#[derive(Debug, ClientStats)]
pub struct BasicClient {
    #[member_start_time]
    pub start_time: time::Instant,
    #[member_valid_jobs]
    pub valid_jobs: stats::CounterUsize,
    #[member_invalid_jobs]
    pub invalid_jobs: stats::CounterUsize,
    #[member_generated_work]
    pub generated_work: CounterU64,
    #[member_last_share]
    pub last_share: LastShare,
    #[member_best_share]
    pub best_share: BestShare,
    #[member_accepted]
    pub accepted: stats::Meter,
    #[member_rejected]
    pub rejected: stats::Meter,
    #[member_stale]
    pub stale: stats::Meter,
    #[member_valid_network_diff]
    pub valid_network_diff: Meter,
    #[member_valid_job_diff]
    pub valid_job_diff: Meter,
    #[member_valid_backend_diff]
    pub valid_backend_diff: Meter,
    #[member_error_backend_diff]
    pub error_backend_diff: Meter,
}

impl BasicClient {
    pub fn new(start_time: time::Instant, intervals: &Vec<time::Duration>) -> Self {
        Self {
            start_time,
            valid_jobs: Default::default(),
            invalid_jobs: Default::default(),
            generated_work: Default::default(),
            last_share: Default::default(),
            best_share: Default::default(),
            accepted: Meter::new(&intervals),
            rejected: Meter::new(&intervals),
            stale: Default::default(),
            valid_network_diff: Meter::new(&intervals),
            valid_job_diff: Meter::new(&intervals),
            valid_backend_diff: Meter::new(&intervals),
            error_backend_diff: Meter::new(&intervals),
        }
    }
}

impl Default for BasicClient {
    fn default() -> Self {
        Self::new(time::Instant::now(), DEFAULT_TIME_MEAN_INTERVALS.as_ref())
    }
}

#[derive(Debug, WorkSolverStats)]
pub struct BasicWorkSolver {
    #[member_start_time]
    pub start_time: time::Instant,
    #[member_last_work_time]
    pub last_work_time: Timestamp,
    #[member_generated_work]
    pub generated_work: CounterU64,
    #[member_last_share]
    pub last_share: LastShare,
    #[member_best_share]
    pub best_share: BestShare,
    #[member_valid_network_diff]
    pub valid_network_diff: Meter,
    #[member_valid_job_diff]
    pub valid_job_diff: Meter,
    #[member_valid_backend_diff]
    pub valid_backend_diff: Meter,
    #[member_error_backend_diff]
    pub error_backend_diff: Meter,
}

impl BasicWorkSolver {
    pub fn new(start_time: time::Instant, intervals: &Vec<time::Duration>) -> Self {
        Self {
            start_time,
            last_share: Default::default(),
            best_share: Default::default(),
            last_work_time: Default::default(),
            generated_work: Default::default(),
            valid_network_diff: Meter::new(&intervals),
            valid_job_diff: Meter::new(&intervals),
            valid_backend_diff: Meter::new(&intervals),
            error_backend_diff: Meter::new(&intervals),
        }
    }
}

impl Default for BasicWorkSolver {
    fn default() -> Self {
        Self::new(time::Instant::now(), DEFAULT_TIME_MEAN_INTERVALS.as_ref())
    }
}

/// Generate share accounting function for a particular difficulty level
/// The function traverses all nodes in the path and accounts the solution in the field specific
/// to the difficulty level given by `solution_target`
macro_rules! account_impl (
    ($name:ident, $field:ident) => (
        pub(crate) async fn $name(
            path: &node::Path,
            solution_target: &ii_bitcoin::Target,
            time: time::Instant,
        ) {
            for node in path {
                node.mining_stats()
                    .$field()
                    .account_solution(solution_target, time)
                    .await;
            }
        }
    )
);

account_impl!(account_valid_network_diff, valid_network_diff);
account_impl!(account_valid_job_diff, valid_job_diff);
account_impl!(account_valid_backend_diff, valid_backend_diff);
account_impl!(account_error_backend_diff, error_backend_diff);

/// Describes which difficulty target a particular solution has met.
/// It also determines in which statistics a particular solution should be accounted.
#[derive(Debug, PartialEq)]
pub enum DiffTargetType {
    Network,
    Job,
    Backend,
}

/// Accounts a valid `solution` to all relevant share accounting statistics based on
/// `met_diff_target_type`. Higher level DiffTargetType also belongs to all lower level types e.g.:
/// - solution that meets DiffTargetType::Network also belongs to DiffTargetType::{Job, Backend}
/// - solution that meets DiffTargetType::Job also belongs to DiffTargetType::Job accounts
pub async fn account_valid_solution(
    path: &node::Path,
    solution: &work::Solution,
    time: time::Instant,
    met_diff_target_type: DiffTargetType,
) {
    account_valid_backend_diff(path, solution.backend_target(), time).await;
    if met_diff_target_type != DiffTargetType::Backend {
        let target = solution.job_target();
        account_valid_job_diff(path, target, time).await;
        if met_diff_target_type != DiffTargetType::Job {
            account_valid_network_diff(path, target, time).await;
            assert_eq!(
                met_diff_target_type,
                DiffTargetType::Network,
                "BUG: unexpected difficulty target type"
            );
        }
        // use only job difficulty for accounting the last share even if a hash of the solution
        // meets higher difficulties
        for node in path {
            let mining_stats = node.mining_stats();
            mining_stats
                .last_share()
                .account_solution(target, time::SystemTime::now())
                .await;
            mining_stats.best_share().account_solution(target);
        }
    }
}

pub async fn mining_task(node: node::DynInfo, interval: time::Duration) {
    loop {
        delay_for(time::Duration::from_secs(1)).await;
        let valid_job_diff = node.mining_stats().valid_job_diff().take_snapshot().await;

        info!(
            "Hash rate ({} s avg.) for '{}' @ pool difficulty: {}/s",
            interval.as_secs(),
            node,
            valid_job_diff.to_pretty_hashes(interval, time::Instant::now())
        );
    }
}
