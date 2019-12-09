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

use std::time;

#[derive(Debug, Clone, Copy)]
struct WindowedTimeMeanState {
    /// Time for the last inserted sample
    last_sample_time: Option<time::Instant>,
    /// Mean value from the previous time window
    prev_window: Option<f64>,
    /// Sum of all samples for the current time window
    sum: f64,
}

impl WindowedTimeMeanState {
    pub fn insert(&mut self, sample: f64, now: time::Instant, interval: f64) -> Option<()> {
        match self.last_sample_time {
            None => self.last_sample_time = Some(now),
            Some(start_time) => {
                let elapsed = now.checked_duration_since(start_time)?.as_secs_f64();
                // check if current window is full
                if elapsed >= interval {
                    // ensure that previous window isn't computed from older history than specified
                    // interval itself
                    self.prev_window = Some(if elapsed < (2.0 * interval) {
                        self.sum / elapsed * interval
                    } else {
                        Default::default()
                    });

                    assert!(now >= start_time, "invalid time for insert");
                    self.last_sample_time = Some(now);
                    self.sum = Default::default();
                }
            }
        }

        self.sum += sample;
        Some(())
    }
}

impl Default for WindowedTimeMeanState {
    fn default() -> Self {
        Self {
            last_sample_time: None,
            prev_window: None,
            sum: Default::default(),
        }
    }
}

/// Calculation of approximate arithmetic mean within given time interval
#[derive(Debug, Clone, Copy)]
pub struct WindowedTimeMean {
    /// Time interval in seconds for arithmetic mean measurement
    interval: f64,
    /// State for incoming samples targeting to the future
    curr_state: WindowedTimeMeanState,
    /// State for incoming samples targeting to the past
    prev_state: WindowedTimeMeanState,
}

impl WindowedTimeMean {
    pub fn new(interval: time::Duration) -> Self {
        assert!(interval.as_secs() > 0);
        Self {
            interval: interval.as_secs_f64(),
            curr_state: Default::default(),
            prev_state: Default::default(),
        }
    }

    #[inline]
    pub fn interval(&self) -> time::Duration {
        time::Duration::from_secs_f64(self.interval)
    }

    /// Measure arithmetic mean at specific time from inserted samples within given time interval.
    /// The specified time shall not be less than time of previous sample.
    pub fn measure(&self, now: time::Instant) -> f64 {
        match self.curr_state.last_sample_time {
            // nothing is measured yet
            None => Default::default(),
            Some(start_time) => {
                assert!(now >= start_time, "invalid time for measurement");
                let elapsed = now.duration_since(start_time).as_secs_f64();
                // check if the windows aren't out of interval
                if elapsed >= (2.0 * self.interval) {
                    return Default::default();
                }
                match self.curr_state.prev_window {
                    // the time interval is still in the first window
                    None => {
                        // avoid division by zero
                        if elapsed == 0.0 {
                            Default::default()
                        } else {
                            self.curr_state.sum / elapsed
                        }
                    }
                    Some(_) if elapsed >= self.interval => {
                        // previous window is out of reach
                        self.curr_state.sum / elapsed
                    }
                    Some(prev_window) => {
                        // compute how far we are into current window, p < 1
                        let distance = elapsed / self.interval;
                        // interpolate between this and previous window
                        (self.curr_state.sum + prev_window * (1.0 - distance)) / self.interval
                    }
                }
            }
        }
    }

    /// Insert another sample for arithmetic mean measurement at specific time.
    /// The specified time shall not be less than time of previous sample.
    pub fn insert(&mut self, sample: f64, now: time::Instant) {
        if self.curr_state.insert(sample, now, self.interval).is_none() {
            let sum_delta = self.curr_state.sum - self.prev_state.sum;
            assert!(sum_delta >= 0.0, "BUG: negative sum delta");
            if self.prev_state.insert(sample, now, self.interval).is_some() {
                // correct current state
                let last_sample_time = self
                    .curr_state
                    .last_sample_time
                    .expect("BUG: missing last sample time");
                self.curr_state = self.prev_state;
                self.curr_state
                    .insert(sum_delta, last_sample_time, self.interval)
                    .expect("BUG: cannot correct current state of windowed time mean");
            }
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use time::Duration;

    #[test]
    fn test_windowed_time_insert_past_time() {
        let start = time::Instant::now();
        let mut mean = WindowedTimeMean::new(Duration::from_secs(3));

        mean.insert(1.0, start);
        mean.insert(1.0, start - Duration::from_secs(1));
    }

    #[test]
    #[should_panic]
    fn test_windowed_time_measure_past_time() {
        let start = time::Instant::now();
        let mut mean = WindowedTimeMean::new(Duration::from_secs(3));

        mean.insert(1.0, start);
        mean.measure(start - Duration::from_secs(1));
    }

    #[test]
    fn test_windowed_time_insert_same_time() {
        let start = time::Instant::now();
        let mut mean = WindowedTimeMean::new(Duration::from_secs(3));

        mean.insert(1.0, start);
        mean.insert(1.0, start);
    }

    #[test]
    fn test_windowed_time_mean_3s() {
        // create simple test measuring 3s interval sampled each 1s
        let start = time::Instant::now();

        let mut mean = WindowedTimeMean::new(Duration::from_secs(3));

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
