//! Closed-loop control runner (sync) — the layered special case where a neural
//! backend (e.g. an Engram network) is "just another controller". A `Controller`
//! turns the latest `SensorFrame` into a `CommandFrame`; a `SafetyGovernor` clamps
//! it; a `ControlTransport` delivers it. (A Python peer mirrors this in its
//! `transport`/`loop` modules.)
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
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).latest_sensor = Some(frame);
    }
    pub fn last_command(&self) -> Option<CommandFrame> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).last_command.clone()
    }
    pub fn commands(&self) -> Vec<CommandFrame> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).commands.clone()
    }
    pub fn statuses(&self) -> Vec<ControlStatus> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).statuses.clone()
    }
}

impl ControlTransport for InProcessTransport {
    fn send_command(&self, command: &CommandFrame) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.last_command = Some(command.clone());
        g.commands.push(command.clone());
    }
    fn latest_sensor(&self) -> Option<SensorFrame> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).latest_sensor.clone()
    }
    fn send_status(&self, status: &ControlStatus) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).statuses.push(status.clone());
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
            ch.insert(
                "velocity_setpoint".into(),
                ChannelValue::vec3(0.0, 0.0, 0.0, Some("m/s")),
            );
            return CommandFrame {
                mode: Mode::Hold,
                channels: ch,
                ..Default::default()
            };
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
        ch.insert(
            "velocity_setpoint".into(),
            ChannelValue {
                data: cmd,
                unit: Some("m/s".into()),
            },
        );
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
    /// Link-health monitor over the inbound sensor `seq` stream — feeds the
    /// HOLD->ESTOP jam escalation (a sustained loss burst latches ESTOP).
    link: crate::resilience::LinkMonitor,
    last_sensor_t: Option<f64>,
    /// Last accepted sensor's `(t, seq)`, to detect a frozen/cached stream. The
    /// watchdog clock (`last_sensor_t`) only advances when the sensor STRICTLY
    /// advances (FIX 4) — a repeated/stale frame must still trip the stale-sensor
    /// HOLD even though a frame "arrived".
    last_sensor_ts: Option<(f64, i64)>,
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
            link: crate::resilience::LinkMonitor::with_defaults("ncp-loop"),
            last_sensor_t: None,
            last_sensor_ts: None,
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
        // FIX 4: only treat the sensor as "fresh" when its (t, seq) STRICTLY
        // advanced. A frozen/cached stream (same t & seq re-delivered) must NOT
        // refresh the watchdog clock, so the stale-sensor HOLD still trips.
        if let Some(s) = sensor.as_ref() {
            let advanced = match self.last_sensor_ts {
                None => true,
                Some((pt, pseq)) => s.t > pt || (s.t == pt && s.seq > pseq),
            };
            if advanced {
                self.last_sensor_t = Some(now);
                self.last_sensor_ts = Some((s.t, s.seq));
                // Feed the link monitor only on a genuinely-new sensor (a frozen
                // re-delivery is a duplicate no-op in the monitor regardless).
                self.link.on_seq(s.seq);
            }
        }
        let mut cmd = self.controller.step(sensor.as_ref(), self.dt_ms());
        // CommandFrame.seq echoes the originating SensorFrame.seq so the split-plane
        // V<->A join pairs the action with the sensor that produced it (the normative
        // invariant; an observer joining V to A on seq depends on it). The loop's own
        // free-running tick counter is carried only on ControlStatus, below.
        if let Some(s) = sensor.as_ref() {
            cmd.seq = s.seq;
        }
        // Escalate to a latched ESTOP if the link monitor reports a jam (a sustained
        // loss burst): a collapsed link must de-energize to safe, not sit in
        // self-clearing HOLD. Checked every tick so the latch persists once tripped.
        self.gov.note_link(self.link.is_burst());
        let mut cmd = self
            .gov
            .govern(&cmd, sensor.as_ref(), now, self.last_sensor_t);
        // Couple the emitted deadline to the loop period: a command must outlive at
        // least the next tick, or a slow (sub-~5 Hz) loop would expire every command
        // before its successor arrives and chatter HOLD on a healthy link.
        cmd.ttl_ms = cmd.ttl_ms.max(self.dt_ms() * 2.0);
        self.transport.send_command(&cmd);
        // loop_latency_ms is a real health field: emit the measured tick cost (not a
        // constant 0.0) and flag an overrun past the loop period in `note`.
        let loop_latency_ms = ((self.now_fn)() - now) * 1000.0;
        self.transport.send_status(&ControlStatus {
            seq: self.seq,
            t: now,
            mode: cmd.mode,
            loop_latency_ms,
            safety_ok: self.gov.safety_ok(),
            note: (loop_latency_ms > self.dt_ms())
                .then(|| format!("overrun: {loop_latency_ms:.1}ms > {:.1}ms", self.dt_ms())),
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
            SafetyLimits {
                max_speed_mps: Some(1.5),
                command_timeout_ms: 500.0,
                ..Default::default()
            },
        )
        .with_clock(Box::new(move || *clock2.lock().unwrap()));

        // No sensor yet -> HOLD.
        let cmd = loop_.tick();
        assert_eq!(cmd.mode, Mode::Hold);

        // Provide a sensor with a position error -> ACTIVE drive back toward origin.
        let mut ch = crate::messages::Map::new();
        ch.insert(
            "pose_position".into(),
            ChannelValue::vec3(1.0, 0.0, 0.0, Some("m")),
        );
        ch.insert(
            "pose_velocity".into(),
            ChannelValue::vec3(0.0, 0.0, 0.0, Some("m/s")),
        );
        transport.push_sensor(SensorFrame {
            channels: ch,
            ..Default::default()
        });
        *clock.lock().unwrap() = 0.05;
        let cmd = loop_.tick();
        assert_eq!(cmd.mode, Mode::Active);
        let v = &cmd.channels["velocity_setpoint"].data;
        assert!(v[0] < 0.0, "should push back toward origin, got {v:?}");
    }

    #[test]
    fn frozen_sensor_trips_stale_hold() {
        // FIX 4: a sensor that keeps arriving with the SAME (t, seq) is a frozen
        // stream; the watchdog clock must not advance, so once the timeout elapses
        // the loop HOLDs even though frames are "arriving" every tick.
        let transport = InProcessTransport::new();
        let clock = Arc::new(Mutex::new(0.0_f64));
        let clock2 = clock.clone();
        let mut loop_ = NeuroControlLoop::new(
            transport.clone(),
            ReflexController::default(),
            20.0,
            SafetyLimits {
                max_speed_mps: Some(1.5),
                command_timeout_ms: 200.0,
                ..Default::default()
            },
        )
        .with_clock(Box::new(move || *clock2.lock().unwrap()));

        // One frozen frame (t=0, seq=0) that we never update.
        let mut ch = crate::messages::Map::new();
        ch.insert(
            "pose_position".into(),
            ChannelValue::vec3(1.0, 0.0, 0.0, Some("m")),
        );
        ch.insert(
            "pose_velocity".into(),
            ChannelValue::vec3(0.0, 0.0, 0.0, Some("m/s")),
        );
        transport.push_sensor(SensorFrame {
            t: 0.0,
            seq: 0,
            channels: ch,
            ..Default::default()
        });

        // First tick at t=0 accepts it -> ACTIVE.
        let cmd = loop_.tick();
        assert_eq!(cmd.mode, Mode::Active, "first fresh frame drives");

        // Advance wall clock well past the 200 ms timeout WITHOUT updating the
        // sensor (same t & seq re-delivered). The frozen stream must go stale.
        *clock.lock().unwrap() = 0.5;
        let cmd = loop_.tick();
        assert_eq!(
            cmd.mode,
            Mode::Hold,
            "a frozen sensor must trip the stale-sensor HOLD"
        );
    }

    #[test]
    fn command_seq_echoes_sensor_seq() {
        // Normative invariant: CommandFrame.seq echoes the originating
        // SensorFrame.seq (so the split-plane V<->A join pairs an action with the
        // sensor that produced it), NOT the loop's free-running tick counter.
        let transport = InProcessTransport::new();
        let clock = Arc::new(Mutex::new(0.0_f64));
        let clock2 = clock.clone();
        let mut loop_ = NeuroControlLoop::new(
            transport.clone(),
            ReflexController::default(),
            20.0,
            SafetyLimits {
                max_speed_mps: Some(1.5),
                command_timeout_ms: 500.0,
                ..Default::default()
            },
        )
        .with_clock(Box::new(move || *clock2.lock().unwrap()));

        // One sensor-less tick advances the loop's internal counter to 1.
        let _ = loop_.tick();

        // A sensor with a distinctive seq=7: the emitted command must carry seq=7
        // (the sensor echo), not 1 (the loop counter).
        let mut ch = crate::messages::Map::new();
        ch.insert(
            "pose_position".into(),
            ChannelValue::vec3(1.0, 0.0, 0.0, Some("m")),
        );
        ch.insert(
            "pose_velocity".into(),
            ChannelValue::vec3(0.0, 0.0, 0.0, Some("m/s")),
        );
        transport.push_sensor(SensorFrame {
            t: 0.1,
            seq: 7,
            channels: ch,
            ..Default::default()
        });
        *clock.lock().unwrap() = 0.05;
        let cmd = loop_.tick();
        assert_eq!(
            cmd.seq, 7,
            "CommandFrame.seq must echo SensorFrame.seq, not the loop tick counter"
        );
    }

    #[test]
    fn link_jam_escalates_to_latched_estop() {
        // A sustained loss burst on the sensor seq stream must latch ESTOP via the
        // loop's LinkMonitor -> SafetyGovernor::note_link escalation (not mere HOLD).
        let transport = InProcessTransport::new();
        let clock = Arc::new(Mutex::new(0.0_f64));
        let clock2 = clock.clone();
        let mut loop_ = NeuroControlLoop::new(
            transport.clone(),
            ReflexController::default(),
            20.0,
            // Huge command_timeout so the stale-sensor HOLD path can't mask the jam.
            SafetyLimits {
                command_timeout_ms: 100_000.0,
                ..Default::default()
            },
        )
        .with_clock(Box::new(move || *clock2.lock().unwrap()));

        let frame = |t: f64, seq: i64| {
            let mut m = crate::messages::Map::new();
            m.insert(
                "pose_position".into(),
                ChannelValue::vec3(0.1, 0.0, 0.0, Some("m")),
            );
            m.insert(
                "pose_velocity".into(),
                ChannelValue::vec3(0.0, 0.0, 0.0, Some("m/s")),
            );
            SensorFrame {
                t,
                seq,
                channels: m,
                ..Default::default()
            }
        };
        for (i, seq) in [0_i64, 1, 2, 50].into_iter().enumerate() {
            let t = (i as f64 + 1.0) * 0.05;
            *clock.lock().unwrap() = t;
            transport.push_sensor(frame(t, seq));
            let cmd = loop_.tick();
            if seq == 50 {
                assert_eq!(
                    cmd.mode,
                    Mode::Estop,
                    "a sensor-seq jam must escalate to ESTOP"
                );
            }
        }
        // Latched: a subsequent clean frame must STILL be ESTOP.
        *clock.lock().unwrap() = 0.30;
        transport.push_sensor(frame(0.30, 51));
        assert_eq!(
            loop_.tick().mode,
            Mode::Estop,
            "jam ESTOP must latch until reset"
        );
    }

    #[test]
    fn loop_latency_ms_is_measured() {
        // A clock advancing per read => the post-send read exceeds the tick-start
        // read, so loop_latency_ms is a real measured value, not a constant 0.0.
        let transport = InProcessTransport::new();
        let clock = Arc::new(Mutex::new(0.0_f64));
        let clock2 = clock.clone();
        let mut loop_ = NeuroControlLoop::new(
            transport.clone(),
            ReflexController::default(),
            20.0,
            SafetyLimits::default(),
        )
        .with_clock(Box::new(move || {
            let mut t = clock2.lock().unwrap();
            *t += 0.001; // +1 ms per read
            *t
        }));
        let _ = loop_.tick();
        let st = transport.statuses().pop().expect("a status was emitted");
        assert!(
            st.loop_latency_ms > 0.0,
            "loop_latency_ms must be a measured value, got {}",
            st.loop_latency_ms
        );
    }
}
