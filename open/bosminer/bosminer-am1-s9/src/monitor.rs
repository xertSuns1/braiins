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

//! This module is responsible for collecting temperatures from hashchains and driving
//! the fans.

use ii_logging::macros::*;

use crate::fan;
use crate::sensor::{self, Measurement};

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::channel::mpsc;
use futures::lock::Mutex;
use futures::stream::StreamExt;
use ii_async_compat::futures;
use ii_async_compat::tokio;
use tokio::timer::delay_for;

/// If miner start takes longer than this, mark it as `Broken`
const START_TIMEOUT: Duration = Duration::from_secs(120);
/// If miner doesn't send temperature update within this time, mark it as dead.
/// This timeout doubles as hashchain watchdog timeout.
/// TODO: Synchronize timeout with temperature monitor task
const RUN_UPDATE_TIMEOUT: Duration = Duration::from_secs(10);
/// How often check timeouts and adjust PID
const TICK_LENGTH: Duration = Duration::from_secs(5);

/// A message from hashchain
///
/// Here are some rules that HashChains registered with monitors have to obey:
///
/// - state change must be strictly `[Off -> On -> Running*]*`
/// - duration between `On` and first `Running` must be less than START_TIMEOUT
/// - duration between `Running` measurement and the next one must be less than
///   RUN_UPDATE_INTERVAL (ideally set periodic update to half of this interval)
#[derive(Debug, Clone)]
pub enum Message {
    On,
    Running(sensor::Temperature),
    Off,
}

/// Interpreted hashchain temperature
#[derive(Debug, Clone, Copy, PartialEq)]
enum ChainTemperature {
    /// Temperature unknown... in a good way (hashchain initializing, etc.)
    Unknown,
    /// Temperature unknown... in a bad way (miner caught fire, etc.)
    Failed,
    /// Temperature was measured
    Ok(f32),
}

impl ChainTemperature {
    /// Convert temperature to monitor interpretation.
    /// Specific to S9, because it fakes chip temperature.
    ///
    /// TODO: Maybe figure out a strage for disabling remote sensors that are failing. Sometimes
    /// remote sensors fail while mining and instead of signalizing error they return non-sensical
    /// numbers.
    fn from_s9_sensor(temp: sensor::Temperature) -> Self {
        match temp.remote {
            Measurement::Ok(t) => Self::Ok(t),
            _ => {
                // fake chip temperature from local temperature
                Self::Ok(temp.local + 15.0)
            }
        }
    }
}

/// State of hashchain as seen from Monitor point of view
/// The `Instant` timestamps are when that event happen (only states that operate with
/// timeouts use it).
#[derive(Debug, Clone, PartialEq)]
enum ChainState {
    On(Instant),
    Running(Instant, sensor::Temperature),
    Off,
    Broken(&'static str),
}

impl ChainState {
    /// Go into invalid state (no way out)
    fn bad_transition(&mut self) {
        *self = ChainState::Broken("bad state transition");
    }

    /// React on an incoming message by changing modifying state. All messages
    /// have follow pattern `[Off -> On -> Running*]*`
    ///
    /// `now` is timestamp of `message` reception (passed explicitly as argument
    /// to facilitate testing).
    fn transition(&mut self, now: Instant, message: Message) {
        match message {
            Message::On => match *self {
                ChainState::Off => *self = ChainState::On(now),
                _ => self.bad_transition(),
            },
            Message::Running(temp) => match *self {
                ChainState::Running(_, _) | ChainState::On(_) => {
                    *self = ChainState::Running(now, temp)
                }
                _ => self.bad_transition(),
            },
            Message::Off => match *self {
                ChainState::Running(_, _) => *self = ChainState::Off,
                _ => self.bad_transition(),
            },
        }
    }

    /// Do a timer tick: check all timeouts and do appropriate state transitions.
    /// If miner is starting, check it starts in `START_TIMEOUT`, if its running, check
    /// it's sending "heartbeats" often enought.
    fn tick(&mut self, now: Instant) {
        match *self {
            ChainState::On(at) => {
                if now - at >= START_TIMEOUT {
                    *self = ChainState::Broken("took too long to start");
                }
            }
            ChainState::Running(at, _) => {
                if now - at >= RUN_UPDATE_TIMEOUT {
                    *self = ChainState::Broken("failed to set update in time");
                }
            }
            _ => {}
        }
    }

    /// Return hashchain temperature as seen from our point of view. For example,
    /// `Broken` miner doesn't have a valid temperature reading even though it sent
    /// some numbers a while ago.
    fn get_temp(&self) -> ChainTemperature {
        match self {
            ChainState::On(_) => ChainTemperature::Unknown,
            ChainState::Off => ChainTemperature::Unknown,
            ChainState::Broken(_) => ChainTemperature::Failed,
            ChainState::Running(_, temp) => ChainTemperature::from_s9_sensor(temp.clone()),
        }
    }
}

/// Represent hashchains as registered within Monitor
struct Chain {
    state: ChainState,
    hashboard_idx: usize,
}

impl Chain {
    fn new(hashboard_idx: usize) -> Self {
        Self {
            state: ChainState::Off,
            hashboard_idx,
        }
    }
}

/// What method of controlling fans is configured
#[derive(Debug, Clone)]
pub enum FanControlMode {
    FixedSpeed(fan::Speed),
    TargetTemperature(f32),
}

/// Fan configuration
#[derive(Debug, Clone)]
pub struct FanControlConfig {
    pub mode: FanControlMode,
    /// Minimal number of fans - miner will refuse to work until at least
    /// this number of fans is spinning.
    pub min_fans: usize,
}

/// Temperature limit configuration
#[derive(Debug, Clone)]
pub struct TempControlConfig {
    pub dangerous_temp: f32,
    pub hot_temp: f32,
}

/// Overall configuration
/// "Disabled" is represented as `None`
#[derive(Debug, Clone)]
pub struct Config {
    pub fan_config: Option<FanControlConfig>,
    pub temp_config: Option<TempControlConfig>,
}

#[derive(Debug, PartialEq)]
pub struct PIDParams {
    target_temp: f32,
    input_temp: f32,
}

/// Output of the decision process
#[derive(Debug, PartialEq)]
pub enum ControlDecision {
    /// Fail state - shutdown miner
    Shutdown(&'static str),
    /// Pass these parameters to PID and let it calculate fan speed
    UsePid(PIDParams),
    /// Use fixed speed
    UseFixedSpeed(fan::Speed),
    /// Do nothing (only valid when fan control is disabled)
    Nothing,
}

impl ControlDecision {
    /// Decision rules if both fan control and temp control are enabled
    fn decide_fan_control(
        fan_config: &FanControlConfig,
        temp_config: &TempControlConfig,
        temp: ChainTemperature,
    ) -> Self {
        match &fan_config.mode {
            FanControlMode::FixedSpeed(pwm) => return Self::UseFixedSpeed(*pwm),
            FanControlMode::TargetTemperature(target_temp) => match temp {
                ChainTemperature::Failed => {
                    panic!("BUG: should've been caught earlier in this function")
                }
                ChainTemperature::Unknown => return Self::UseFixedSpeed(fan::Speed::FULL_SPEED),
                ChainTemperature::Ok(input_temp) => {
                    if input_temp >= temp_config.hot_temp {
                        return Self::UseFixedSpeed(fan::Speed::FULL_SPEED);
                    }
                    return Self::UsePid(PIDParams {
                        target_temp: *target_temp,
                        input_temp,
                    });
                }
            },
        }
    }

    /// Decision rules if fan control is enabled and temp control disabled
    fn decide_fan_control_notemp(fan_config: &FanControlConfig) -> Self {
        match fan_config.mode {
            FanControlMode::FixedSpeed(pwm) => return Self::UseFixedSpeed(pwm),
            FanControlMode::TargetTemperature(_) => {
                // I don't know how to avoid this variant using type system alone
                // Let's make it non-fatal
                return Self::UseFixedSpeed(fan::Speed::FULL_SPEED);
            }
        }
    }

    /// Decide what to do depending on temperature/fan feedback.
    /// This function has been factored out of the main control code to facilitate testing.
    fn decide(config: &Config, num_fans: usize, temp: ChainTemperature) -> Self {
        // Check for dangerous temperature or dead sensors
        if let Some(temp_config) = config.temp_config.as_ref() {
            match temp {
                ChainTemperature::Failed => {
                    return Self::Shutdown("temperature readout failed");
                }
                ChainTemperature::Ok(input_temp) => {
                    if input_temp >= temp_config.dangerous_temp {
                        return Self::Shutdown("temperature dangerous");
                    }
                }
                ChainTemperature::Unknown => {}
            }
        }
        // Check the health of fans and decide their speed
        if let Some(fan_config) = config.fan_config.as_ref() {
            let decision = if let Some(temp_config) = config.temp_config.as_ref() {
                Self::decide_fan_control(fan_config, temp_config, temp)
            } else {
                Self::decide_fan_control_notemp(fan_config)
            };
            // Check `min_fans` are spinning _unless_ we have been explicitly configured to
            // turn them off.
            //
            // XXX: There's a problem however: if we are configured for stopped fans and then
            // the configuration changes at runtime to non-stopped fans, the delay of fans
            // taking some time to spin up will cause this check to fire off!
            if decision != Self::UseFixedSpeed(fan::Speed::STOPPED) {
                if num_fans < fan_config.min_fans {
                    return Self::Shutdown("not enough fans");
                }
            }
            decision
        } else {
            // This is only valid if `FanControl` is turned off
            Self::Nothing
        }
    }
}

/// This structure abstracts the process of "making one aggregate temperature out of
/// all hashchain temperatures".
/// The resulting temperature is used as an input variable for PID control.
#[derive(Debug)]
struct TemperatureAccumulator {
    pub temp: ChainTemperature,
}

impl TemperatureAccumulator {
    /// Start in unknown state
    fn new() -> Self {
        Self {
            temp: ChainTemperature::Unknown,
        }
    }

    /// Function to calculate aggregated temperature.
    /// This one calculates maximum temperatures over all temperatures measured while
    /// prefering failures to measurement.
    fn add_chain_temp(&mut self, chain_temp: ChainTemperature) {
        self.temp = match chain_temp {
            // Failure trumphs everything
            ChainTemperature::Failed => chain_temp,
            // Unknown doesn't add any information - no change
            ChainTemperature::Unknown => self.temp,
            ChainTemperature::Ok(t1) => {
                match self.temp {
                    // Failure trumphs everything
                    ChainTemperature::Failed => self.temp,
                    // Take maximum of temperatures
                    ChainTemperature::Ok(t2) => ChainTemperature::Ok(t1.max(t2)),
                    ChainTemperature::Unknown => ChainTemperature::Ok(t1),
                }
            }
        };
    }
}

/// Monitor - it holds states of all Chains and everything related to fan control
pub struct Monitor {
    /// Each chain is registered here
    chains: Vec<Arc<Mutex<Chain>>>,
    /// temp/fan control configuration
    config: Config,
    /// Fan controller - can set RPM or read feedback
    fan_control: fan::Control,
    /// PID that controls fan with hashchain temperature as input
    pid: fan::pid::TempControl,
}

impl Monitor {
    pub fn new(config: Config) -> Arc<Mutex<Self>> {
        let monitor = Arc::new(Mutex::new(Self {
            chains: Vec::new(),
            config,
            fan_control: fan::Control::new().expect("failed initializing fan controller"),
            pid: fan::pid::TempControl::new(),
        }));

        tokio::spawn(Self::tick_task(monitor.clone()));

        // start tasks etc
        monitor
    }

    /// Shutdown miner
    /// TODO: do a more graceful shutdown
    fn shutdown(&self) {
        panic!("Monitor task declared miner shutdown");
    }

    /// Set fan speed
    fn set_fan_speed(&self, fan_speed: fan::Speed) {
        info!("Monitor: {:?}", fan_speed);
        self.fan_control.set_speed(fan_speed);
    }

    /// Task performing temp control
    async fn tick_task(monitor: Arc<Mutex<Self>>) {
        loop {
            // TODO: find some of kind "run every x secs" function
            delay_for(TICK_LENGTH).await;

            // decide hashchain state and collect temperatures
            let mut monitor = monitor.lock().await;
            let mut acc = TemperatureAccumulator::new();
            for chain in monitor.chains.iter() {
                let mut chain = chain.lock().await;
                chain.state.tick(Instant::now());

                if let ChainState::Broken(reason) = chain.state {
                    // TODO: here comes "Shutdown"
                    error!("Chain {} is broken: {}", chain.hashboard_idx, reason);
                    monitor.shutdown();
                }
                acc.add_chain_temp(chain.state.get_temp());
            }

            // Read fans
            let fan_feedback = monitor.fan_control.read_feedback();
            let num_fans = fan_feedback.num_fans_running();
            info!(
                "fan={:?} num_fans={} acc={:?}",
                fan_feedback, num_fans, acc.temp
            );

            // all right, temperature has been aggregated, decide what to do
            let decision = ControlDecision::decide(&monitor.config, num_fans, acc.temp);
            info!("decision={:?}", decision);
            match decision {
                ControlDecision::Shutdown(reason) => {
                    error!("Monitor: {}", reason);
                    monitor.shutdown();
                }
                ControlDecision::UseFixedSpeed(fan_speed) => {
                    monitor.set_fan_speed(fan_speed);
                }
                ControlDecision::UsePid(PIDParams {
                    target_temp,
                    input_temp,
                }) => {
                    monitor.pid.set_target(target_temp.into());
                    let speed = monitor.pid.update(input_temp.into());
                    info!(
                        "Monitor: input={} target={} output={:?}",
                        input_temp, target_temp, speed
                    );
                    monitor.set_fan_speed(speed);
                }
                ControlDecision::Nothing => {}
            }
        }
    }

    /// Per-chain task that collects hashchain status update messages
    async fn recv_task(chain: Arc<Mutex<Chain>>, mut rx: mpsc::UnboundedReceiver<Message>) {
        while let Some(message) = rx.next().await {
            let mut chain = chain.lock().await;
            chain.state.transition(Instant::now(), message);
        }
    }

    /// Registers hashchain within monitor
    /// The `hashboard_idx` parameter is for debugging purposes
    pub async fn register_hashchain(
        monitor: Arc<Mutex<Self>>,
        hashboard_idx: usize,
    ) -> mpsc::UnboundedSender<Message> {
        let chain = Arc::new(Mutex::new(Chain::new(hashboard_idx)));
        {
            let mut monitor = monitor.lock().await;
            monitor.chains.push(chain.clone());
        }
        let (tx, rx) = mpsc::unbounded();
        tokio::spawn(Self::recv_task(chain, rx));
        tx
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use approx::relative_eq;

    macro_rules! assert_variant {
        ($value:expr, $pattern:pat) => {{
            let value = &$value;
            if let $pattern = value {
            } else {
                panic!(
                    r#"assertion failed (value doesn't match pattern):
                        value: `{:?}`,
                        pattern: `{}`"#,
                    value,
                    stringify!($pattern)
                )
            }
        }}; // TODO: Additional patterns for trailing args, like assert and assert_eq
    }

    /// Test that faking S9 chip temperature from board temperature works
    #[test]
    fn test_monitor_s9_chip_temp() {
        let temp = sensor::Temperature {
            local: 10.0,
            remote: sensor::Measurement::Ok(22.0),
        };
        match ChainTemperature::from_s9_sensor(temp) {
            ChainTemperature::Ok(t) => relative_eq!(t, 22.0),
            _ => panic!("missing temperature"),
        };
        let temp = sensor::Temperature {
            local: 10.0,
            remote: sensor::Measurement::OpenCircuit,
        };
        match ChainTemperature::from_s9_sensor(temp) {
            ChainTemperature::Ok(t) => relative_eq!(t, 25.0),
            _ => panic!("missing temperature"),
        };
    }

    fn send(mut state: ChainState, when: Instant, message: Message) -> ChainState {
        state.transition(when, message);
        state
    }

    /// Test that miner transitions states as expected
    #[test]
    fn test_monitor_state_transition() {
        let temp = sensor::Temperature {
            local: 10.0,
            remote: sensor::Measurement::Ok(22.0),
        };
        let now = Instant::now();
        let later = now + Duration::from_secs(1);

        //assert_eq!(send(ChainState::Running(now, temp), later, Message::Off), ChainState::Off);
        assert_variant!(send(ChainState::Off, later, Message::On), ChainState::On(_));
        assert_variant!(
            send(ChainState::Off, later, Message::Running(temp.clone())),
            ChainState::Broken(_)
        );
        assert_variant!(
            send(ChainState::Off, later, Message::Off),
            ChainState::Broken(_)
        );

        assert_variant!(
            send(ChainState::On(now), later, Message::On),
            ChainState::Broken(_)
        );
        assert_variant!(
            send(ChainState::On(now), later, Message::Running(temp.clone())),
            ChainState::Running(_, _)
        );
        assert_variant!(
            send(ChainState::On(now), later, Message::Off),
            ChainState::Broken(_)
        );

        assert_variant!(
            send(ChainState::Running(now, temp.clone()), later, Message::On),
            ChainState::Broken(_)
        );
        assert_variant!(
            send(
                ChainState::Running(now, temp.clone()),
                later,
                Message::Running(temp.clone())
            ),
            ChainState::Running(_, _)
        );
        assert_variant!(
            send(ChainState::Running(now, temp.clone()), later, Message::Off),
            ChainState::Off
        );
    }

    fn tick(mut state: ChainState, later: Instant) -> ChainState {
        state.tick(later);
        state
    }

    /// Test timeouts
    #[test]
    fn test_monitor_timeouts() {
        let temp = sensor::Temperature {
            local: 10.0,
            remote: sensor::Measurement::Ok(22.0),
        };
        let now = Instant::now();
        let long = now + Duration::from_secs(10_000);
        let short = now + Duration::from_secs(2);

        // test that chains break when no-one updates them for long (unless they are turned off)
        assert_variant!(tick(ChainState::Off, long), ChainState::Off);
        assert_variant!(tick(ChainState::On(now), long), ChainState::Broken(_));
        assert_variant!(
            tick(ChainState::Running(now, temp.clone()), long),
            ChainState::Broken(_)
        );

        // passing of short time is OK
        assert_variant!(tick(ChainState::Off, short), ChainState::Off);
        assert_variant!(tick(ChainState::On(now), short), ChainState::On(_));
        assert_variant!(
            tick(ChainState::Running(now, temp.clone()), short),
            ChainState::Running(_, _)
        );

        // different states have different update timeouts
        assert_variant!(
            tick(ChainState::On(now), now + Duration::from_secs(20)),
            ChainState::On(_)
        );
        assert_variant!(
            tick(
                ChainState::Running(now, temp.clone()),
                now + Duration::from_secs(20)
            ),
            ChainState::Broken(_)
        );
    }
}
