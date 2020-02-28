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

//! This crate is intended for various statistical algorithms used mainly for mining.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
struct WindowedTimeMeanState {
    /// Window interval
    interval: f64,
    /// Time for the last inserted sample
    started: Option<Instant>,
    /// Mean value from the previous time window
    prev_window: f64,
    /// Sum of all samples for the current time window
    sum: f64,
}

impl WindowedTimeMeanState {
    pub fn new(interval: f64) -> Self {
        Self {
            interval,
            started: None,
            prev_window: 0.0,
            sum: 0.0,
        }
    }

    pub fn insert(&mut self, sample: f64, now: Instant) {
        match self.started {
            None => {
                self.started = Some(now);
                self.sum = sample;
                self.prev_window = 0.0;
            }
            Some(start_time) => {
                let elapsed = now
                    .checked_duration_since(start_time)
                    .expect("BUG: non-monotonic clock")
                    .as_secs_f64();
                // check if current window is full
                if elapsed >= self.interval {
                    // ensure that previous window isn't computed from older history than specified
                    // self.interval itself
                    let a = elapsed / self.interval;
                    self.prev_window = if a < 2.0 { self.sum / a } else { 0.0 };
                    self.started = Some(now);
                    self.sum = 0.0;
                }
                self.sum += sample;
            }
        }
    }

    pub fn measure(&self, now: Instant) -> f64 {
        match self.started {
            None => 0.0,
            Some(start_time) => {
                let elapsed = now
                    .checked_duration_since(start_time)
                    .expect("BUG: non-monotonic clock")
                    .as_secs_f64();

                let a = elapsed / self.interval;
                let sum = if a < 1.0 {
                    self.prev_window * (1.0 - a) + self.sum * a
                } else {
                    self.sum
                };
                sum / self.interval
            }
        }
    }
}

/// Calculation of approximate arithmetic mean within given time interval
#[derive(Debug, Clone, Copy)]
pub struct WindowedTimeMean {
    /// State for incoming samples targeting to the future
    state: WindowedTimeMeanState,
}

impl WindowedTimeMean {
    pub fn new(interval: Duration) -> Self {
        assert!(interval.as_secs() > 0);
        Self {
            state: WindowedTimeMeanState::new(interval.as_secs_f64()),
        }
    }

    #[inline]
    pub fn interval(&self) -> Duration {
        Duration::from_secs_f64(self.state.interval)
    }

    /// Measure arithmetic mean at specific time from inserted samples within given time interval.
    /// TODO: do not ignore time
    pub fn measure(&self, _now: Instant) -> f64 {
        self.state.measure(Instant::now())
    }

    /// Insert another sample for arithmetic mean measurement at specific time.
    /// TODO: do not ignore time
    pub fn insert(&mut self, sample: f64, _now: Instant) {
        self.state.insert(sample, Instant::now());
    }
}

#[cfg(test)]
pub mod test {
    use super::*;

    #[test]
    fn test_windowed_time_insert_same_time() {
        let start = Instant::now();
        let mut mean = WindowedTimeMeanState::new(3.0);

        mean.insert(1.0, start);
        mean.insert(1.0, start);
    }

    #[test]
    #[ignore]
    fn test_windowed_time_mean_3s() {
        // create simple test measuring 3s interval sampled each 1s
        let start = Instant::now();

        let mut mean = WindowedTimeMeanState::new(3.0);

        // check if mean is equal to 0 when any sample was not inserted yet
        assert_eq!(mean.measure(start), 0.0);
        assert_eq!(mean.measure(start + Duration::from_secs(1)), 0.0);

        // check average measurement of the sequence [1, 2, 3]
        mean.insert(1.0, start + Duration::from_secs(1));
        assert_eq!(mean.measure(start + Duration::from_secs(2)), 1.0);
        mean.insert(2.0, start + Duration::from_secs(2));
        assert_eq!(mean.measure(start + Duration::from_secs(3)), 1.5);
        mean.insert(3.0, start + Duration::from_secs(3));
        assert_eq!(mean.measure(start + Duration::from_secs(4)), 2.0);
        assert_eq!(mean.measure(start + Duration::from_secs(5)), 1.5);
        assert_eq!(mean.measure(start + Duration::from_secs(6)), 1.2);
        assert_eq!(mean.measure(start + Duration::from_secs(7)), 0.0);

        mean.insert(1.0, start + Duration::from_secs(7));
        assert_eq!(
            mean.measure(start + Duration::from_secs(8)),
            0.3333333333333333
        );
        mean.insert(2.0, start + Duration::from_secs(8));
        assert_eq!(mean.measure(start + Duration::from_secs(9)), 1.0);
        mean.insert(3.0, start + Duration::from_secs(9));
        assert_eq!(mean.measure(start + Duration::from_secs(10)), 2.0);
        assert_eq!(mean.measure(start + Duration::from_secs(11)), 1.5);
        assert_eq!(mean.measure(start + Duration::from_secs(12)), 1.2);
        assert_eq!(mean.measure(start + Duration::from_secs(13)), 0.0);
        assert_eq!(mean.measure(start + Duration::from_secs(14)), 0.0);

        mean.insert(1.0, start + Duration::from_secs(14));
        assert_eq!(
            mean.measure(start + Duration::from_secs(15)),
            0.3333333333333333
        );
        mean.insert(2.0, start + Duration::from_secs(15));
        assert_eq!(mean.measure(start + Duration::from_secs(16)), 1.0);

        // insert at the same time as previous measurement
        mean.insert(3.0, start + Duration::from_secs(16));
        // and repeat it again
        assert_eq!(mean.measure(start + Duration::from_secs(16)), 2.0);
        assert_eq!(mean.measure(start + Duration::from_secs(17)), 2.0);
        assert_eq!(mean.measure(start + Duration::from_secs(18)), 1.5);
    }
}
