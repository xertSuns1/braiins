use crate::hal;

use crate::misc::LOGGER;
use slog::{info, trace};

use tokio::await;
use tokio::timer::Delay;

use futures_locks::Mutex;

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Dummy difficulty
const ASIC_DIFFICULTY: u64 = 1;

fn shares_to_ghs(shares: u128, diff: u128) -> f32 {
    (shares * (diff << 32)) as f32 * 1e-9_f32
}

pub async fn hashrate_meter_task(mining_stats: Arc<Mutex<hal::MiningStats>>) {
    let hashing_started = SystemTime::now();
    let mut total_shares: u128 = 0;

    loop {
        await!(Delay::new(Instant::now() + Duration::from_secs(1))).unwrap();
        let mut stats = await!(mining_stats.lock()).expect("lock mining stats");
        {
            let unique_solutions = stats.unique_solutions as u128;
            stats.unique_solutions = 0;
            total_shares = total_shares + unique_solutions;
            // processing solution in the test simply means removing them

            let total_hashing_time = hashing_started.elapsed().expect("time read ok");

            trace!(
                LOGGER,
                "Hash rate: {} Gh/s (immediate {} Gh/s)",
                shares_to_ghs(total_shares, ASIC_DIFFICULTY as u128)
                    / total_hashing_time.as_secs() as f32,
                shares_to_ghs(unique_solutions, ASIC_DIFFICULTY as u128) as f32,
            );
            info!(
                LOGGER,
                "Total_shares: {}, total_time: {} s, total work generated: {}",
                total_shares,
                total_hashing_time.as_secs(),
                stats.work_generated,
            );
            trace!(
                LOGGER,
                "Mismatched nonce count: {}, stale solutions: {}, duplicate solutions: {}",
                stats.mismatched_solution_nonces,
                stats.stale_solutions,
                stats.duplicate_solutions,
            );
        }
    }
}
