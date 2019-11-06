// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU Common Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Common Public License for more details.
//
// You should have received a copy of the GNU Common Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

use pid_control::{Controller, PIDController};
use std::time::Instant;

/// TODO: maybe separate into own module?
struct OffsetedPIDController {
    pid: PIDController,
    offset: f64,
}

impl OffsetedPIDController {
    pub fn new(p_gain: f64, i_gain: f64, d_gain: f64, offset: f64) -> Self {
        Self {
            pid: PIDController::new(p_gain, i_gain, d_gain),
            offset,
        }
    }

    pub fn set_limits(&mut self, min: f64, max: f64) {
        self.pid.set_limits(min - self.offset, max - self.offset);
    }
}

impl Controller for OffsetedPIDController {
    fn set_target(&mut self, target: f64) {
        self.pid.set_target(target);
    }

    fn target(&self) -> f64 {
        self.pid.target()
    }

    fn update(&mut self, value: f64, delta_t: f64) -> f64 {
        self.pid.update(value, delta_t) + self.offset
    }

    fn reset(&mut self) {
        self.pid.reset()
    }
}

pub struct TempControl {
    pid: OffsetedPIDController,
    last_update: Instant,
}

impl TempControl {
    pub fn new() -> Self {
        // kp/ki/kd constants are negative because the PID works in reverse direction
        // (the lower the PWM, the higher the temperature)
        let mut pid = OffsetedPIDController::new(-5.0, -0.03, -0.015, 70.0);
        pid.set_limits(0.0, 100.0);

        Self {
            pid,
            last_update: Instant::now(),
        }
    }

    pub fn set_target(&mut self, target: f64) {
        self.pid.set_target(target);
    }

    pub fn update(&mut self, temperature: f64) -> f64 {
        let pwm = self
            .pid
            .update(temperature, self.last_update.elapsed().as_secs_f64());
        self.last_update = Instant::now();
        pwm
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Verify that offset is added to the results of PID controller computation
    #[test]
    fn test_pid_offset() {
        let mut pid = OffsetedPIDController::new(0.0, 0.0, 0.0, 50.0);
        assert_eq!(pid.update(0.0, 1.0), 50.0);
        pid.set_limits(60.0, 60.0);
        assert_eq!(pid.update(0.0, 1.0), 60.0);
    }
}
