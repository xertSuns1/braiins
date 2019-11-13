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
/// How long does it take until miner warm up? We won't let it tu turn fans off until then...
const WARM_UP_PERIOD: Duration = Duration::from_secs(90);

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
pub enum ChainTemperature {
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
    Running {
        started: Instant,
        last_heartbeat: Instant,
        temperature: sensor::Temperature,
    },
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
            Message::Running(temperature) => match *self {
                ChainState::Running { started, .. } | ChainState::On(started) => {
                    *self = ChainState::Running {
                        started,
                        last_heartbeat: now,
                        temperature,
                    }
                }
                _ => self.bad_transition(),
            },
            Message::Off => match *self {
                ChainState::Running { .. } => *self = ChainState::Off,
                _ => self.bad_transition(),
            },
        }
    }

    /// Do a timer tick: check all timeouts and do appropriate state transitions.
    /// If miner is starting, check it starts in `START_TIMEOUT`, if its running, check
    /// it's sending "heartbeats" often enought.
    fn tick(&mut self, now: Instant) {
        match *self {
            ChainState::On(started) => {
                if now.duration_since(started) >= START_TIMEOUT {
                    *self = ChainState::Broken("took too long to start");
                }
            }
            ChainState::Running { last_heartbeat, .. } => {
                if now.duration_since(last_heartbeat) >= RUN_UPDATE_TIMEOUT {
                    *self = ChainState::Broken("failed to set update in time");
                }
            }
            _ => {}
        }
    }

    /// Return hashchain temperature as seen from our point of view. For example,
    /// `Broken` miner doesn't have a valid temperature reading even though it sent
    /// some numbers a while ago.
    fn get_temperature(&self) -> ChainTemperature {
        match self {
            ChainState::On(_) => ChainTemperature::Unknown,
            ChainState::Off => ChainTemperature::Unknown,
            ChainState::Broken(_) => ChainTemperature::Failed,
            ChainState::Running { temperature, .. } => {
                ChainTemperature::from_s9_sensor(temperature.clone())
            }
        }
    }

    /// Is hashchain warming up?
    fn is_warming_up(&self, now: Instant) -> bool {
        match self {
            // chain state stays in "warming up" state until it sends heartbeat
            ChainState::On(_) => true,
            ChainState::Running { started, .. } => now.duration_since(*started) <= WARM_UP_PERIOD,
            _ => false,
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

/// Output of the decision process
#[derive(Debug, PartialEq)]
pub enum ControlDecision {
    /// Fail state - shutdown miner
    Shutdown(&'static str),
    /// Pass these parameters to PID and let it calculate fan speed
    UsePid { target_temp: f32, input_temp: f32 },
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
        if temp == ChainTemperature::Unknown {
            return Self::UseFixedSpeed(fan::Speed::FULL_SPEED);
        }
        match &fan_config.mode {
            FanControlMode::FixedSpeed(pwm) => return Self::UseFixedSpeed(*pwm),
            FanControlMode::TargetTemperature(target_temp) => match temp {
                ChainTemperature::Failed | ChainTemperature::Unknown => {
                    panic!("BUG: should've been caught earlier in this function")
                }
                ChainTemperature::Ok(input_temp) => {
                    if input_temp >= temp_config.hot_temp {
                        return Self::UseFixedSpeed(fan::Speed::FULL_SPEED);
                    }
                    return Self::UsePid {
                        target_temp: *target_temp,
                        input_temp,
                    };
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
    fn decide(config: &Config, num_fans_running: usize, temp: ChainTemperature) -> Self {
        // This section is labeled `TEMP_DANGER` in the diagram
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
            // This section is labeled `FAN_DANGER` in the diagram
            //
            // Check `min_fans` are spinning _unless_ we have been explicitly configured to
            // turn them off.
            //
            // XXX: There's a problem however: if we are configured for stopped fans and then
            // the configuration changes at runtime to non-stopped fans, the delay of fans
            // taking some time to spin up will cause this check to fire off!
            if decision != Self::UseFixedSpeed(fan::Speed::STOPPED) {
                if num_fans_running < fan_config.min_fans {
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
    pub config: Config,
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
    fn shutdown(&self, reason: String) {
        panic!(format!("Monitor task declared miner shutdown: {}", reason));
    }

    /// Set fan speed
    fn set_fan_speed(&self, fan_speed: fan::Speed) {
        info!("Monitor: setting fan to {:?}", fan_speed);
        self.fan_control.set_speed(fan_speed);
    }

    pub async fn get_chain_temperatures(monitor: Arc<Mutex<Self>>) -> Vec<ChainTemperature> {
        let mut temperatures = Vec::new();
        let monitor = monitor.lock().await;
        for chain in monitor.chains.iter() {
            let chain = chain.lock().await;
            temperatures.push(chain.state.get_temperature());
        }
        temperatures
    }

    /// Task performing temp control
    async fn tick_task(monitor: Arc<Mutex<Self>>) {
        loop {
            // TODO: find some of kind "run every x secs" function
            delay_for(TICK_LENGTH).await;

            // decide hashchain state and collect temperatures
            let mut monitor = monitor.lock().await;
            let mut cumulative_temperature = TemperatureAccumulator::new();
            let mut miner_warming_up = false;
            for chain in monitor.chains.iter() {
                let mut chain = chain.lock().await;
                chain.state.tick(Instant::now());

                if let ChainState::Broken(reason) = chain.state {
                    // TODO: here comes "Shutdown"
                    monitor.shutdown(format!(
                        "Chain {} is broken: {}",
                        chain.hashboard_idx, reason
                    ));
                }
                info!("chain {}: {:?}", chain.hashboard_idx, chain.state);
                cumulative_temperature.add_chain_temp(chain.state.get_temperature());
                miner_warming_up |= chain.state.is_warming_up(Instant::now());
            }

            // Read fans
            let fan_feedback = monitor.fan_control.read_feedback();
            let num_fans_running = fan_feedback.num_fans_running();
            info!(
                "Monitor: fan={:?} num_fans={} acc.temp.={:?}",
                fan_feedback, num_fans_running, cumulative_temperature.temp,
            );

            // all right, temperature has been aggregated, decide what to do
            let decision = ControlDecision::decide(
                &monitor.config,
                num_fans_running,
                cumulative_temperature.temp,
            );
            info!("Monitor: decision={:?}", decision);
            match decision {
                ControlDecision::Shutdown(reason) => {
                    monitor.shutdown(reason.into());
                }
                ControlDecision::UseFixedSpeed(fan_speed) => {
                    monitor.set_fan_speed(fan_speed);
                }
                ControlDecision::UsePid {
                    target_temp,
                    input_temp,
                } => {
                    if miner_warming_up {
                        monitor.pid.set_warm_up_limits();
                    } else {
                        monitor.pid.set_normal_limits();
                    }
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
        let running_state = ChainState::Running {
            started: now,
            last_heartbeat: now,
            temperature: temp.clone(),
        };

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
            ChainState::Running{ .. }
        );
        assert_variant!(
            send(ChainState::On(now), later, Message::Off),
            ChainState::Broken(_)
        );

        assert_variant!(
            send(running_state.clone(), later, Message::On),
            ChainState::Broken(_)
        );
        assert_variant!(
            send(
                running_state.clone(),
                later,
                Message::Running(temp.clone())
            ),
            ChainState::Running { .. }
        );
        assert_variant!(
            send(running_state.clone(), later, Message::Off),
            ChainState::Off
        );
    }

    /// Test "warm up" period
    #[test]
    fn test_monitor_warm_up() {
        let temp = sensor::Temperature {
            local: 10.0,
            remote: sensor::Measurement::Ok(22.0),
        };
        let now = Instant::now();
        let later = now + Duration::from_secs(20);
        let warmed_time = now + Duration::from_secs(200);
        let running_state = ChainState::Running {
            started: now,
            last_heartbeat: now,
            temperature: temp.clone(),
        };

        assert_eq!(ChainState::Off.is_warming_up(now), false);
        assert_eq!(ChainState::On(now).is_warming_up(now), true);
        assert_eq!(ChainState::On(now).is_warming_up(warmed_time), true);
        assert_eq!(running_state.clone().is_warming_up(now), true);
        assert_eq!(running_state.clone().is_warming_up(later), true);
        assert_eq!(running_state.clone().is_warming_up(warmed_time), false);
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
        let running_state = ChainState::Running {
            started: now,
            last_heartbeat: now,
            temperature: temp.clone(),
        };

        // test that chains break when no-one updates them for long (unless they are turned off)
        assert_variant!(tick(ChainState::Off, long), ChainState::Off);
        assert_variant!(tick(ChainState::On(now), long), ChainState::Broken(_));
        assert_variant!(tick(running_state.clone(), long), ChainState::Broken(_));

        // passing of short time is OK
        assert_variant!(tick(ChainState::Off, short), ChainState::Off);
        assert_variant!(tick(ChainState::On(now), short), ChainState::On(_));
        assert_variant!(
            tick(running_state.clone(), short),
            ChainState::Running{..}
        );

        // different states have different update timeouts
        assert_variant!(
            tick(ChainState::On(now), now + Duration::from_secs(20)),
            ChainState::On(_)
        );
        assert_variant!(
            tick(running_state.clone(), now + Duration::from_secs(20)),
            ChainState::Broken(_)
        );
    }

    fn test_acc(temp1: ChainTemperature, temp2: ChainTemperature) -> ChainTemperature {
        let mut state = TemperatureAccumulator { temp: temp1 };
        state.add_chain_temp(temp2);
        state.temp
    }

    /// Test temperature accumulator
    #[test]
    fn test_monitor_temp_acc() {
        assert_eq!(
            test_acc(ChainTemperature::Unknown, ChainTemperature::Unknown),
            ChainTemperature::Unknown
        );
        assert_eq!(
            test_acc(ChainTemperature::Failed, ChainTemperature::Unknown),
            ChainTemperature::Failed
        );
        assert_eq!(
            test_acc(ChainTemperature::Ok(10.0), ChainTemperature::Unknown),
            ChainTemperature::Ok(10.0)
        );
        assert_eq!(
            test_acc(ChainTemperature::Unknown, ChainTemperature::Failed),
            ChainTemperature::Failed
        );
        assert_eq!(
            test_acc(ChainTemperature::Failed, ChainTemperature::Failed),
            ChainTemperature::Failed
        );
        assert_eq!(
            test_acc(ChainTemperature::Ok(10.0), ChainTemperature::Failed),
            ChainTemperature::Failed
        );
        assert_eq!(
            test_acc(ChainTemperature::Unknown, ChainTemperature::Ok(20.0)),
            ChainTemperature::Ok(20.0)
        );
        assert_eq!(
            test_acc(ChainTemperature::Failed, ChainTemperature::Ok(20.0)),
            ChainTemperature::Failed
        );
        assert_eq!(
            test_acc(ChainTemperature::Ok(10.0), ChainTemperature::Ok(20.0)),
            ChainTemperature::Ok(20.0)
        );
        assert_eq!(
            test_acc(ChainTemperature::Ok(10.0), ChainTemperature::Ok(5.0)),
            ChainTemperature::Ok(10.0)
        );
    }

    /// Test temperature decision tree (non-exhaustive test)
    #[test]
    fn test_decide() {
        let dang_temp = ChainTemperature::Ok(150.0);
        let hot_temp = ChainTemperature::Ok(95.0);
        let low_temp = ChainTemperature::Ok(50.0);
        let temp_config = TempControlConfig {
            dangerous_temp: 100.0,
            hot_temp: 80.0,
        };
        let fan_speed = fan::Speed::new(50);
        let fan_config = FanControlConfig {
            mode: FanControlMode::FixedSpeed(fan_speed),
            min_fans: 2,
        };
        let fans_off = fan::Speed::STOPPED;
        let fans_off_config = Config {
            fan_config: Some(FanControlConfig {
                mode: FanControlMode::FixedSpeed(fans_off),
                min_fans: 2,
            }),
            temp_config: None,
        };
        let all_off_config = Config {
            fan_config: None,
            temp_config: None,
        };
        let fans_on_config = Config {
            fan_config: Some(fan_config.clone()),
            temp_config: None,
        };
        let temp_on_config = Config {
            fan_config: None,
            temp_config: Some(temp_config.clone()),
        };
        let both_on_config = Config {
            fan_config: Some(fan_config.clone()),
            temp_config: Some(temp_config.clone()),
        };
        let both_on_pid_config = Config {
            fan_config: Some(FanControlConfig {
                mode: FanControlMode::TargetTemperature(75.0),
                min_fans: 2,
            }),
            temp_config: Some(temp_config.clone()),
        };

        assert_variant!(
            ControlDecision::decide(&all_off_config, 0, dang_temp.clone()),
            ControlDecision::Nothing
        );
        assert_variant!(
            ControlDecision::decide(&all_off_config, 0, ChainTemperature::Failed),
            ControlDecision::Nothing
        );

        assert_eq!(
            ControlDecision::decide(&fans_on_config, 2, dang_temp.clone()),
            ControlDecision::UseFixedSpeed(fan_speed)
        );
        assert_variant!(
            ControlDecision::decide(&fans_on_config, 0, dang_temp.clone()),
            ControlDecision::Shutdown(_)
        );
        assert_variant!(
            ControlDecision::decide(&fans_on_config, 1, dang_temp.clone()),
            ControlDecision::Shutdown(_)
        );
        assert_eq!(
            ControlDecision::decide(&fans_on_config, 2, ChainTemperature::Failed),
            ControlDecision::UseFixedSpeed(fan_speed)
        );

        // fans set to 0 -> do not check if fans are running
        assert_eq!(
            ControlDecision::decide(&fans_off_config, 0, dang_temp.clone()),
            ControlDecision::UseFixedSpeed(fans_off)
        );

        assert_variant!(
            ControlDecision::decide(&temp_on_config, 0, ChainTemperature::Failed),
            ControlDecision::Shutdown(_)
        );
        assert_variant!(
            ControlDecision::decide(&temp_on_config, 0, ChainTemperature::Unknown),
            ControlDecision::Nothing
        );
        assert_variant!(
            ControlDecision::decide(&temp_on_config, 0, dang_temp),
            ControlDecision::Shutdown(_)
        );
        assert_variant!(
            ControlDecision::decide(&temp_on_config, 0, hot_temp),
            ControlDecision::Nothing
        );

        assert_variant!(
            ControlDecision::decide(&both_on_config, 0, low_temp),
            ControlDecision::Shutdown(_)
        );
        assert_variant!(
            ControlDecision::decide(&both_on_config, 2, dang_temp),
            ControlDecision::Shutdown(_)
        );
        assert_variant!(
            ControlDecision::decide(&both_on_config, 2, ChainTemperature::Failed),
            ControlDecision::Shutdown(_)
        );
        assert_eq!(
            ControlDecision::decide(&both_on_config, 2, ChainTemperature::Unknown),
            ControlDecision::UseFixedSpeed(fan::Speed::FULL_SPEED)
        );
        assert_eq!(
            ControlDecision::decide(&both_on_config, 2, hot_temp),
            ControlDecision::UseFixedSpeed(fan_speed)
        );
        assert_eq!(
            ControlDecision::decide(&both_on_config, 2, low_temp),
            ControlDecision::UseFixedSpeed(fan_speed)
        );

        assert_variant!(
            ControlDecision::decide(&both_on_pid_config, 0, low_temp),
            ControlDecision::Shutdown(_)
        );
        assert_variant!(
            ControlDecision::decide(&both_on_pid_config, 2, dang_temp),
            ControlDecision::Shutdown(_)
        );
        assert_variant!(
            ControlDecision::decide(&both_on_pid_config, 2, ChainTemperature::Failed),
            ControlDecision::Shutdown(_)
        );
        assert_eq!(
            ControlDecision::decide(&both_on_pid_config, 2, ChainTemperature::Unknown),
            ControlDecision::UseFixedSpeed(fan::Speed::FULL_SPEED)
        );
        assert_eq!(
            ControlDecision::decide(&both_on_pid_config, 2, hot_temp),
            ControlDecision::UseFixedSpeed(fan::Speed::FULL_SPEED)
        );
        assert_eq!(
            ControlDecision::decide(&both_on_pid_config, 2, low_temp),
            ControlDecision::UsePid {
                target_temp: 75.0,
                input_temp: 50.0
            }
        );
    }
}
