//! Closed-loop control runner (sync) — the layered special case where Engram is
//! "just another controller". A `Controller` turns the latest `SensorFrame` into
//! a `CommandFrame`; a `SafetyGovernor` clamps it; a `ControlTransport` delivers
//! it. Mirrors `backend/neurocontrol/{transport,loop}.py`.
//!
//! Clocks are injectable so the loop is deterministic under test.

use crate::messages::{ChannelValue, CommandFrame, ControlStatus, Mode, SafetyLimits, SensorFrame};
use crate::safety::SafetyGovernor;
use std::sync::{Arc, Mutex};

/// Moves sensor/command frames between a controller and a plant.
pub trait ControlTransport: Send + Sync {
    fn send_command(&self, command: &CommandFrame);
    fn latest_sensor(&self) -> Option<SensorFrame>;
    fn send_status(&self, _status: &ControlStatus) {}
}

/// Bidirectional in-process channel (tests / co-process SITL). The plant calls
/// `push_sensor` / `last_command`; the controller uses `ControlTransport`.
#[derive(Clone, Default)]
pub struct InProcessTransport {
    inner: Arc<Mutex<InProcessInner>>,
}

#[derive(Default)]
struct InProcessInner {
    latest_sensor: Option<SensorFrame>,
    last_command: Option<CommandFrame>,
    commands: Vec<CommandFrame>,
    statuses: Vec<ControlStatus>,
}

impl InProcessTransport {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push_sensor(&self, frame: SensorFrame) {
        self.inner.lock().unwrap().latest_sensor = Some(frame);
    }
    pub fn last_command(&self) -> Option<CommandFrame> {
        self.inner.lock().unwrap().last_command.clone()
    }
    pub fn commands(&self) -> Vec<CommandFrame> {
        self.inner.lock().unwrap().commands.clone()
    }
    pub fn statuses(&self) -> Vec<ControlStatus> {
        self.inner.lock().unwrap().statuses.clone()
    }
}

impl ControlTransport for InProcessTransport {
    fn send_command(&self, command: &CommandFrame) {
        let mut g = self.inner.lock().unwrap();
        g.last_command = Some(command.clone());
        g.commands.push(command.clone());
    }
    fn latest_sensor(&self) -> Option<SensorFrame> {
        self.inner.lock().unwrap().latest_sensor.clone()
    }
    fn send_status(&self, status: &ControlStatus) {
        self.inner.lock().unwrap().statuses.push(status.clone());
    }
}

/// Turns the latest sensor into a command each tick.
pub trait Controller: Send {
    fn reset(&mut self) {}
    fn step(&mut self, sensor: Option<&SensorFrame>, dt_ms: f64) -> CommandFrame;
}

/// Deterministic PD reflex (`velocity_setpoint = -kp*(pos-target) - kd*vel`).
/// The fixed-wiring baseline a trained SNN controller must beat.
pub struct ReflexController {
    pub target: [f64; 3],
    pub kp: f64,
    pub kd: f64,
    pub max_speed: f64,
    pub position_channel: String,
    pub velocity_channel: String,
}

impl Default for ReflexController {
    fn default() -> Self {
        Self {
            target: [0.0, 0.0, 0.0],
            kp: 1.0,
            kd: 0.3,
            max_speed: 1.5,
            position_channel: "pose_position".into(),
            velocity_channel: "pose_velocity".into(),
        }
    }
}

impl Controller for ReflexController {
    fn step(&mut self, sensor: Option<&SensorFrame>, _dt_ms: f64) -> CommandFrame {
        let Some(sensor) = sensor else {
            let mut ch = crate::messages::Map::new();
            ch.insert("velocity_setpoint".into(), ChannelValue::vec3(0.0, 0.0, 0.0, Some("m/s")));
            return CommandFrame { mode: Mode::Hold, channels: ch, ..Default::default() };
        };
        let get3 = |name: &str| -> [f64; 3] {
            let mut out = [0.0; 3];
            if let Some(cv) = sensor.channels.get(name) {
                for (i, slot) in out.iter_mut().enumerate() {
                    *slot = cv.data.get(i).copied().unwrap_or(0.0);
                }
            }
            out
        };
        let p = get3(&self.position_channel);
        let v = get3(&self.velocity_channel);
        let mut cmd = Vec::with_capacity(3);
        for i in 0..3 {
            let u = -self.kp * (p[i] - self.target[i]) - self.kd * v[i];
            cmd.push(u.clamp(-self.max_speed, self.max_speed));
        }
        let mut ch = crate::messages::Map::new();
        ch.insert("velocity_setpoint".into(), ChannelValue { data: cmd, unit: Some("m/s".into()) });
        CommandFrame {
            t: sensor.t,
            seq: sensor.seq,
            frame_id: sensor.frame_id.clone(),
            mode: Mode::Active,
            channels: ch,
            ..Default::default()
        }
    }
}

/// Fixed-rate scheduler tying transport + controller + safety together. `now_fn`
/// is injectable so the loop is deterministic under test.
pub struct NeuroControlLoop<T: ControlTransport, C: Controller> {
    pub transport: T,
    pub controller: C,
    pub rate_hz: f64,
    gov: SafetyGovernor,
    now_fn: Box<dyn Fn() -> f64 + Send>,
    seq: i64,
    last_sensor_t: Option<f64>,
}

impl<T: ControlTransport, C: Controller> NeuroControlLoop<T, C> {
    pub fn new(transport: T, controller: C, rate_hz: f64, safety: SafetyLimits) -> Self {
        Self {
            transport,
            controller,
            rate_hz,
            gov: SafetyGovernor::new(safety),
            now_fn: Box::new(monotonic_secs),
            seq: 0,
            last_sensor_t: None,
        }
    }

    /// Override the clock (tests).
    pub fn with_clock(mut self, now_fn: Box<dyn Fn() -> f64 + Send>) -> Self {
        self.now_fn = now_fn;
        self
    }

    fn dt_ms(&self) -> f64 {
        1000.0 / self.rate_hz
    }

    /// One control step: read sensor → controller → safety → send.
    pub fn tick(&mut self) -> CommandFrame {
        let now = (self.now_fn)();
        let sensor = self.transport.latest_sensor();
        if sensor.is_some() {
            self.last_sensor_t = Some(now);
        }
        let mut cmd = self.controller.step(sensor.as_ref(), self.dt_ms());
        cmd.seq = self.seq;
        let cmd = self.gov.govern(&cmd, sensor.as_ref(), now, self.last_sensor_t);
        self.transport.send_command(&cmd);
        self.transport.send_status(&ControlStatus {
            seq: self.seq,
            t: now,
            mode: cmd.mode,
            safety_ok: cmd.mode != Mode::Estop,
            ..Default::default()
        });
        self.seq += 1;
        cmd
    }
}

fn monotonic_secs() -> f64 {
    use std::time::Instant;
    thread_local! { static START: Instant = Instant::now(); }
    START.with(|s| s.elapsed().as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflex_loop_holds_without_sensor_then_drives() {
        let transport = InProcessTransport::new();
        let controller = ReflexController::default();
        let clock = Arc::new(Mutex::new(0.0_f64));
        let clock2 = clock.clone();
        let mut loop_ = NeuroControlLoop::new(
            transport.clone(),
            controller,
            20.0,
            SafetyLimits { max_speed_mps: Some(1.5), command_timeout_ms: 500.0, ..Default::default() },
        )
        .with_clock(Box::new(move || *clock2.lock().unwrap()));

        // No sensor yet -> HOLD.
        let cmd = loop_.tick();
        assert_eq!(cmd.mode, Mode::Hold);

        // Provide a sensor with a position error -> ACTIVE drive back toward origin.
        let mut ch = crate::messages::Map::new();
        ch.insert("pose_position".into(), ChannelValue::vec3(1.0, 0.0, 0.0, Some("m")));
        ch.insert("pose_velocity".into(), ChannelValue::vec3(0.0, 0.0, 0.0, Some("m/s")));
        transport.push_sensor(SensorFrame { channels: ch, ..Default::default() });
        *clock.lock().unwrap() = 0.05;
        let cmd = loop_.tick();
        assert_eq!(cmd.mode, Mode::Active);
        let v = &cmd.channels["velocity_setpoint"].data;
        assert!(v[0] < 0.0, "should push back toward origin, got {v:?}");
    }
}
