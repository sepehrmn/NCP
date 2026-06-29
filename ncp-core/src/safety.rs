//! Safety governor for the **action plane** — the only plane with command
//! authority. Enforces the parts of `SafetyLimits` a controller can: HOLD on a
//! stale sensor, **latch** ESTOP on a geofence breach, and clamp speed. Returns a
//! *fresh* `CommandFrame` (never mutates the input). `max_tilt_rad` is advisory —
//! the plant / flight controller enforces it. Mirrors `loop.py::SafetyGovernor`.
//!
//! ESTOP **latches**: once any condition trips it, every subsequent `govern`
//! returns ESTOP + a zeroed command until a supervisor calls
//! [`reset`](SafetyGovernor::reset). HOLD (on a
//! stale/frozen sensor) is **non-latching** — it clears as soon as fresh data
//! flows again.
//!
//! The watchdog (`govern` with `last_sensor_s = None` or stale) is the
//! producer-overrun backstop: if the brain (`nest.Run`) misses the deadline, the
//! plant-side governor fails safe to HOLD independent of the controller.

use crate::messages::{Capabilities, ChannelValue, CommandFrame, Mode, SafetyLimits, SensorFrame};

#[derive(Clone, Debug)]
pub struct SafetyGovernor {
    pub limits: SafetyLimits,
    /// Channel carrying the plant position (geofence input). Resolved from the
    /// negotiated `Capabilities`, not hardcoded.
    position_channel: String,
    /// Channel carrying the commanded velocity (speed clamp target).
    velocity_channel: String,
    /// All negotiated command channels — the HOLD/zero path zeroes their union
    /// with the inbound command's channels, never just one literal name.
    command_channels: Vec<String>,
    /// Latched emergency-stop. Set by any ESTOP-tripping condition; cleared only
    /// by [`reset`](SafetyGovernor::reset). While set, every `govern` returns a zeroed ESTOP frame.
    estop: bool,
    /// Latched config-level fail-closed: a limit (geofence/speed) was set whose
    /// channel is absent from the negotiated specs. Per FIX 3 the governor then
    /// HOLDs and reports `safety_ok=false`. A misconfiguration cannot be fixed at
    /// runtime, so it does not clear on [`reset`](SafetyGovernor::reset).
    config_fail_closed: bool,
}

impl Default for SafetyGovernor {
    fn default() -> Self {
        Self {
            limits: SafetyLimits::default(),
            position_channel: "pose_position".to_string(),
            velocity_channel: "velocity_setpoint".to_string(),
            command_channels: vec!["velocity_setpoint".to_string()],
            estop: false,
            config_fail_closed: false,
        }
    }
}

impl SafetyGovernor {
    /// Construct with default channel wiring (`pose_position` / `velocity_setpoint`).
    /// Prefer [`from_capabilities`](SafetyGovernor::from_capabilities) so the enforced channels track the negotiated
    /// handshake.
    pub fn new(limits: SafetyLimits) -> Self {
        Self {
            limits,
            ..Default::default()
        }
    }

    /// Construct with explicitly resolved channel names. `command_channels` is the
    /// negotiated set of command-plane channels the HOLD/zero path must zero.
    /// `sensor_channels` is the negotiated perception-plane set, used only to
    /// validate that a configured geofence's position channel actually exists. If
    /// a limit references a channel absent from these specs the governor starts in
    /// a latched config fail-closed state (FIX 3).
    pub fn with_channels(
        limits: SafetyLimits,
        position_channel: impl Into<String>,
        velocity_channel: impl Into<String>,
        command_channels: Vec<String>,
        sensor_channels: Vec<String>,
    ) -> Self {
        let position_channel = position_channel.into();
        let velocity_channel = velocity_channel.into();
        let command_channels = if command_channels.is_empty() {
            vec![velocity_channel.clone()]
        } else {
            command_channels
        };
        let config_fail_closed = Self::detect_misconfig(
            &limits,
            &position_channel,
            &velocity_channel,
            &command_channels,
            &sensor_channels,
        );
        Self {
            limits,
            position_channel,
            velocity_channel,
            command_channels,
            estop: false,
            config_fail_closed,
        }
    }

    /// A geofence/speed limit whose channel is not declared in the negotiated
    /// specs is a misconfiguration that must fail closed rather than silently
    /// no-op. Position is checked against the sensor specs; speed against the
    /// command specs.
    fn detect_misconfig(
        limits: &SafetyLimits,
        position_channel: &str,
        velocity_channel: &str,
        command_channels: &[String],
        sensor_channels: &[String],
    ) -> bool {
        let geofence_bad = limits.geofence_radius_m.is_some_and(|r| r > 0.0)
            && !sensor_channels.is_empty()
            && !sensor_channels.iter().any(|c| c == position_channel);
        let speed_bad = limits.max_speed_mps.is_some_and(|s| s > 0.0)
            && !command_channels.iter().any(|c| c == velocity_channel);
        geofence_bad || speed_bad
    }

    /// Resolve the enforced channels from the negotiated [`Capabilities`]. The
    /// position channel is the first sensor channel (falling back to
    /// `pose_position`); the velocity channel is the first command channel
    /// (falling back to `velocity_setpoint`); the HOLD/zero set is every declared
    /// command channel. Geofence/speed limits come from `caps.safety`. A limit
    /// referencing an undeclared channel starts the governor fail-closed.
    pub fn from_capabilities(caps: &Capabilities) -> Self {
        let command_channels: Vec<String> = caps
            .command_channels
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let sensor_channels: Vec<String> = caps
            .sensor_channels
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let velocity_channel = command_channels
            .first()
            .cloned()
            .unwrap_or_else(|| "velocity_setpoint".to_string());
        let position_channel = sensor_channels
            .first()
            .cloned()
            .unwrap_or_else(|| "pose_position".to_string());
        Self::with_channels(
            caps.safety.clone(),
            position_channel,
            velocity_channel,
            command_channels,
            sensor_channels,
        )
    }

    /// Clear a latched ESTOP. Only a supervisor calls this — after a fresh govern
    /// the latch may re-engage if the tripping condition still holds. A config-level
    /// fail-closed latch (undeclared limit channel) is NOT cleared: it is an
    /// unrecoverable misconfiguration, not a transient breach.
    pub fn reset(&mut self) {
        self.estop = false;
    }

    /// True while ESTOP is latched.
    pub fn is_estopped(&self) -> bool {
        self.estop
    }

    /// Latch ESTOP when the link monitor reports a sustained loss burst (a jam) —
    /// the documented Layer-3 fail-safe escalation. A collapsed link is NOT a
    /// transient HOLD; it de-energizes to a latched safe state until a supervisor
    /// [`reset`](SafetyGovernor::reset)s (an operator-supplied loss-rate threshold may gate this too, but
    /// the CUSUM `burst` is the trip today). Without this, a jammed craft sits in
    /// self-clearing HOLD forever while the link is dead.
    pub fn note_link(&mut self, burst: bool) {
        if burst {
            self.estop = true; // latch
        }
    }

    /// Whether the last governed command was safe. False under a latched ESTOP or a
    /// config-level fail-closed (undeclared limit channel). The loop reports this
    /// in `ControlStatus.safety_ok`.
    pub fn safety_ok(&self) -> bool {
        !self.estop && !self.config_fail_closed
    }

    /// Zero the union of the inbound command's channels and the negotiated command
    /// channels, preserving each channel's arity (width) and unit so the HOLD/ESTOP
    /// frame is shaped exactly like the live command — no channel is silently
    /// dropped or left unzeroed.
    fn zeroed_channels(&self, command: &CommandFrame) -> crate::messages::Map<ChannelValue> {
        let mut m = crate::messages::Map::new();
        for (name, cv) in &command.channels {
            m.insert(
                name.clone(),
                ChannelValue {
                    data: vec![0.0; cv.data.len().max(1)],
                    unit: cv.unit.clone(),
                },
            );
        }
        for name in &self.command_channels {
            m.entry(name.clone()).or_insert_with(|| {
                if name == &self.velocity_channel {
                    ChannelValue::vec3(0.0, 0.0, 0.0, Some("m/s"))
                } else {
                    ChannelValue::scalar(0.0, None)
                }
            });
        }
        m
    }

    fn estop_frame(&self, command: &CommandFrame) -> CommandFrame {
        CommandFrame {
            t: command.t,
            seq: command.seq,
            mode: Mode::Estop,
            channels: self.zeroed_channels(command),
            ..Default::default()
        }
    }

    fn hold_frame(&self, command: &CommandFrame) -> CommandFrame {
        CommandFrame {
            t: command.t,
            seq: command.seq,
            mode: Mode::Hold,
            channels: self.zeroed_channels(command),
            ..Default::default()
        }
    }

    /// Apply safety to `command`. `now_s` and `last_sensor_s` are wall-clock
    /// seconds; a missing/old sensor forces HOLD (fail-safe to zero, **not**
    /// latch-last). ESTOP **latches**: once tripped, every later call returns a
    /// zeroed ESTOP until [`reset`](SafetyGovernor::reset). Takes `&mut self` because of that latch.
    pub fn govern(
        &mut self,
        command: &CommandFrame,
        sensor: Option<&SensorFrame>,
        now_s: f64,
        last_sensor_s: Option<f64>,
    ) -> CommandFrame {
        // Latched ESTOP dominates everything until a supervisor reset.
        if self.estop {
            return self.estop_frame(command);
        }
        // Config-level fail-closed (a limit references an undeclared channel):
        // HOLD every command; `safety_ok()` reports false.
        if self.config_fail_closed {
            return self.hold_frame(command);
        }

        // Default-deny on a bad command_timeout_ms: a non-finite / zero / negative
        // timeout is treated as "always stale" (HOLD), never "never stale". Note
        // `f64::NAN.max(0.0) == 0.0`, so the old `.max(0.0)` pre-clamp turned NaN
        // into a never-stale 0 — fail-open. Compare the raw ms value instead.
        let timeout_ms = self.limits.command_timeout_ms;
        let timeout_s = timeout_ms / 1000.0;
        let stale = match last_sensor_s {
            None => true,
            Some(last) => {
                // A non-finite clock (NaN/±inf `now_s` or `last`) makes the
                // `(now_s - last) > timeout_s` comparison NaN→false — i.e. "not
                // stale", a fail-OPEN on a bad clock. Treat any non-finite clock
                // input as stale (HOLD), defaulting the staleness backstop closed.
                !now_s.is_finite()
                    || !last.is_finite()
                    || !timeout_ms.is_finite()
                    || timeout_ms <= 0.0
                    || !timeout_s.is_finite()
                    || (now_s - last) > timeout_s
            }
        };
        if stale {
            // HOLD is non-latching — do NOT set self.estop.
            return self.hold_frame(command);
        }

        // Geofence: if a positive radius is configured we MUST be able to evaluate
        // it. An absent position channel (sensor missing it, or no sensor) is a
        // fail-closed condition, not a silent no-op.
        if let Some(radius) = self.limits.geofence_radius_m {
            if radius > 0.0 {
                let pos = sensor.and_then(|s| s.channels.get(&self.position_channel));
                match pos {
                    None => {
                        // Cannot evaluate the fence -> fail closed. HOLD (non-latching:
                        // the channel may reappear) with safety_ok=false at the caller.
                        return self.hold_frame(command);
                    }
                    Some(pos) => {
                        // An empty position vector (e.g. a declared vec3 channel that
                        // arrives with no data) makes r = sqrt(0) = 0 — "at the origin",
                        // inside any fence — silently bypassing the geofence. Treat
                        // missing data like an absent channel: fail closed (HOLD,
                        // non-latching since the data may reappear).
                        if pos.data.is_empty() {
                            return self.hold_frame(command);
                        }
                        let r = pos.data.iter().map(|c| c * c).sum::<f64>().sqrt();
                        // A non-finite `r` (NaN from upstream) fails safe to ESTOP
                        // rather than silently passing the `r > radius` comparison.
                        if !r.is_finite() || r > radius {
                            self.estop = true; // latch
                            return self.estop_frame(command);
                        }
                    }
                }
            }
        }

        let mut out = command.clone();
        if let Some(max_speed) = self.limits.max_speed_mps {
            if max_speed > 0.0 {
                // Tick 0: an absent or non-finite velocity cannot be enforced ->
                // fail closed (HOLD).
                if self.clamp_velocity(&mut out.channels, max_speed).is_err() {
                    return self.hold_frame(command);
                }
                // CRITICAL: clamp every predictive horizon step too. The
                // ActionBuffer replays `horizon[i]` verbatim on every tick after
                // 0, so an unclamped horizon defeats the speed limit for the whole
                // ride-through window. A step that cannot be clamped (absent /
                // non-finite velocity) truncates the horizon there, so replay
                // HOLDs rather than emitting an unbounded setpoint.
                let mut safe_len = out.horizon.len();
                for (i, step) in out.horizon.iter_mut().enumerate() {
                    if self.clamp_velocity(step, max_speed).is_err() {
                        safe_len = i;
                        break;
                    }
                }
                out.horizon.truncate(safe_len);
            }
        }

        // Geofence horizon look-ahead: the speed-clamped horizon is replayed
        // open-loop through a dropout, so if the plant is within one horizon's worth
        // of travel of the fence, that replay could cross it unchecked (the tick-0
        // check above only guards the current position). Truncate the horizon when
        // near the fence so replay HOLDs; tick-0 still actuates. (When here, the
        // geofence block above has already ensured `r <= radius` and a finite `r`.)
        if let Some(radius) = self.limits.geofence_radius_m {
            if radius > 0.0 && !out.horizon.is_empty() {
                if let Some(pos) = sensor.and_then(|s| s.channels.get(&self.position_channel)) {
                    let r = pos.data.iter().map(|c| c * c).sum::<f64>().sqrt();
                    let dt_s = command.horizon_dt_ms.unwrap_or(0.0) / 1000.0;
                    let n = out.horizon.len() as f64;
                    // Max distance the open-loop horizon can carry the plant from `r`.
                    // Unbounded speed + a horizon => cannot bound the excursion => drop it.
                    let margin = match self.limits.max_speed_mps {
                        Some(v) if v > 0.0 && dt_s > 0.0 => v * n * dt_s,
                        _ => f64::INFINITY,
                    };
                    if r.is_finite() && r > radius - margin {
                        out.horizon.clear();
                    }
                }
            }
        }
        out
    }

    /// Clamp the velocity channel of `channels` to `max_speed` (m/s), in place,
    /// preserving direction and unit. `Ok(())` if it was within the limit or
    /// successfully scaled down; `Err(())` if the velocity channel is absent or
    /// its magnitude is non-finite — i.e. the limit cannot be enforced and the
    /// caller must fail safe. Shared by the tick-0 command and every horizon step
    /// so the speed bound holds across the entire predictive replay.
    fn clamp_velocity(
        &self,
        channels: &mut crate::messages::Map<ChannelValue>,
        max_speed: f64,
    ) -> Result<(), ()> {
        let vel = channels.get(&self.velocity_channel).ok_or(())?;
        let mag = vel.data.iter().map(|c| c * c).sum::<f64>().sqrt();
        if !mag.is_finite() {
            return Err(());
        }
        if mag > max_speed {
            let k = max_speed / mag;
            let data: Vec<f64> = vel.data.iter().map(|c| c * k).collect();
            let unit = vel.unit.clone();
            channels.insert(self.velocity_channel.clone(), ChannelValue { data, unit });
        }
        Ok(())
    }
}

/// Plant-side deadline backstop that **enforces `CommandFrame.ttl_ms`** — which is
/// otherwise carried on the wire but never checked. Feed each accepted command's
/// **local** arrival time and its `ttl_ms`; the plant must fail safe (HOLD to a
/// zero/safe setpoint) once the latest command has expired or none has arrived.
/// Using the plant's own clock avoids controller↔plant clock skew. This is the
/// deadline backstop the packetized-predictive-control horizon (see RESILIENCE.md)
/// relies on: replay buffered predictions only while unexpired, HOLD on drain.
/// Upper bound on an enforced command ttl. The wire field `ttl_ms` is unbounded,
/// but the plant-side deadline backstop must stay finite: an absurdly large (or
/// `+Inf`) ttl would let a single command keep the plant "live" indefinitely,
/// defeating the watchdog. 60 s is far beyond any real control deadline.
const MAX_TTL_MS: f64 = 60_000.0;

#[derive(Clone, Debug, Default)]
pub struct CommandWatchdog {
    last_recv_s: Option<f64>,
    ttl_s: f64,
    /// Highest accepted command `seq`. The deadline refreshes only on a strictly
    /// advancing seq, so a stale/duplicate command cannot extend liveness.
    last_seq: i64,
}

impl CommandWatchdog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an accepted command received at local time `now_s` with its `ttl_ms`
    /// and `seq`. The deadline refreshes only when `seq` strictly advances — a
    /// duplicate/stale/replayed command (`seq <= last`) must NOT extend liveness, or
    /// a trickle of stale frames would keep the plant "fresh" forever. `seq == 0` is
    /// the all-zero-seq escape hatch (pull/sim streams that do not stamp seq).
    pub fn on_command(&mut self, now_s: f64, ttl_ms: f64, seq: i64) {
        if seq != 0 && seq <= self.last_seq {
            return; // stale/duplicate command does not refresh the deadline
        }
        if seq != 0 {
            self.last_seq = seq;
        }
        self.last_recv_s = Some(now_s);
        // Bound the enforced ttl: a non-finite `ttl_ms` (e.g. `+Inf`) makes
        // `(now - t) > ttl_s` never true → the backstop never fires (fail-OPEN).
        // Map non-finite to 0 (immediately stale) and clamp very large values to a
        // finite ceiling. The wire still carries `ttl_ms` unchanged.
        self.ttl_s = if ttl_ms.is_finite() {
            ttl_ms.clamp(0.0, MAX_TTL_MS) / 1000.0
        } else {
            0.0
        };
    }

    /// True if the plant must fail safe to HOLD: no command yet, or the latest is
    /// past its ttl. (A non-positive ttl is treated as immediately stale.)
    pub fn should_hold(&self, now_s: f64) -> bool {
        match self.last_recv_s {
            None => true,
            // A non-finite clock (`now_s` or the stored `t`) makes
            // `(now_s - t) > ttl_s` evaluate NaN→false — "not expired", a
            // fail-OPEN backstop on a bad clock. Treat any non-finite clock as
            // expired (HOLD), so the deadline backstop defaults closed.
            Some(t) => {
                !now_s.is_finite()
                    || !t.is_finite()
                    || self.ttl_s <= 0.0
                    || (now_s - t) > self.ttl_s
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_watchdog_enforces_ttl() {
        let mut wd = CommandWatchdog::new();
        assert!(wd.should_hold(0.0), "no command yet -> HOLD");
        wd.on_command(1.0, 200.0, 1); // ttl 200 ms
        assert!(!wd.should_hold(1.1), "within ttl -> apply");
        assert!(wd.should_hold(1.3), "0.3 s > 0.2 s ttl -> HOLD");
    }

    #[test]
    fn duplicate_command_does_not_extend_ttl() {
        let mut wd = CommandWatchdog::new();
        wd.on_command(1.0, 200.0, 5); // accepted; ttl 200 ms
                                      // Stale/duplicate commands (seq <= 5) must NOT refresh the deadline.
        wd.on_command(1.15, 200.0, 5); // duplicate
        wd.on_command(1.15, 200.0, 3); // older
        assert!(
            wd.should_hold(1.25),
            "deadline anchored at the seq=5 command; stale frames must not extend it"
        );
        // A strictly-advancing command refreshes it.
        wd.on_command(1.2, 200.0, 6);
        assert!(!wd.should_hold(1.3), "seq=6 advances -> refreshed");
        // The all-zero-seq escape hatch still refreshes (pull/sim streams).
        wd.on_command(1.5, 200.0, 0);
        assert!(!wd.should_hold(1.6), "seq=0 stream still refreshes");
    }

    #[test]
    fn unbounded_or_nonfinite_ttl_still_expires() {
        // +Inf ttl must NOT mean "never stale" (that would let one command keep
        // the plant live forever — fail-OPEN). It is clamped to a finite ceiling.
        let mut wd = CommandWatchdog::new();
        wd.on_command(0.0, f64::INFINITY, 1);
        assert!(
            wd.should_hold(MAX_TTL_MS / 1000.0 + 1.0),
            "a +Inf ttl must still expire past the finite ceiling"
        );
        // NaN ttl -> immediately stale (fail-safe).
        let mut wd2 = CommandWatchdog::new();
        wd2.on_command(0.0, f64::NAN, 1);
        assert!(
            wd2.should_hold(0.001),
            "a NaN ttl is treated as immediately stale"
        );
    }

    #[test]
    fn empty_position_holds_under_geofence() {
        let mut gov = SafetyGovernor::new(SafetyLimits {
            geofence_radius_m: Some(10.0),
            ..Default::default()
        });
        // A declared position channel that arrives with no data must not read as
        // r = sqrt(0) = 0 ("at the origin", inside the fence) — fail closed to HOLD.
        let mut ch = crate::messages::Map::new();
        ch.insert(
            "pose_position".to_string(),
            ChannelValue {
                data: vec![],
                unit: Some("m".into()),
            },
        );
        let sensor = SensorFrame {
            channels: ch,
            ..Default::default()
        };
        let out = gov.govern(&CommandFrame::default(), Some(&sensor), 1.0, Some(1.0));
        assert_eq!(
            out.mode,
            Mode::Hold,
            "an empty position vector must fail closed (HOLD), not bypass the geofence"
        );
    }

    #[test]
    fn note_link_burst_latches_estop() {
        let mut gov = SafetyGovernor::new(SafetyLimits::default());
        assert!(!gov.is_estopped());
        gov.note_link(false); // no jam -> no change
        assert!(!gov.is_estopped());
        gov.note_link(true); // jam burst -> latch
        assert!(gov.is_estopped(), "a link burst must latch ESTOP");
        gov.note_link(false); // a later clear link must NOT un-latch
        assert!(gov.is_estopped(), "ESTOP latch must persist until reset");
        gov.reset();
        assert!(!gov.is_estopped());
    }

    #[test]
    fn geofence_horizon_lookahead_truncates_near_fence() {
        // radius 10 m, max_speed 5 m/s, horizon 4 steps @ 100 ms => margin 5*4*0.1 = 2 m.
        let mut gov = SafetyGovernor::new(SafetyLimits {
            geofence_radius_m: Some(10.0),
            max_speed_mps: Some(5.0),
            command_timeout_ms: 500.0,
            ..Default::default()
        });
        let cmd = CommandFrame {
            mode: Mode::Active,
            channels: channels_with("velocity_setpoint", 1.0, "m/s"),
            horizon: vec![
                channels_with("velocity_setpoint", 1.0, "m/s"),
                channels_with("velocity_setpoint", 1.0, "m/s"),
                channels_with("velocity_setpoint", 1.0, "m/s"),
                channels_with("velocity_setpoint", 1.0, "m/s"),
            ],
            horizon_dt_ms: Some(100.0),
            ..Default::default()
        };
        // r=9: inside the fence but within the 2 m horizon margin (9 > 10-2).
        let near = SensorFrame {
            channels: channels_with("pose_position", 9.0, "m"),
            ..Default::default()
        };
        let out = gov.govern(&cmd, Some(&near), 1.0, Some(1.0));
        assert_eq!(
            out.mode,
            Mode::Active,
            "tick-0 inside the fence still actuates"
        );
        assert!(
            out.horizon.is_empty(),
            "near the fence the open-loop horizon must be truncated"
        );
        // r=3: well inside (3 < 10-2) -> horizon preserved.
        let inside = SensorFrame {
            channels: channels_with("pose_position", 3.0, "m"),
            ..Default::default()
        };
        let out2 = gov.govern(&cmd, Some(&inside), 2.0, Some(2.0));
        assert_eq!(
            out2.horizon.len(),
            4,
            "well inside the fence the horizon is kept"
        );
    }

    fn channels_with(name: &str, x: f64, unit: &str) -> crate::messages::Map<ChannelValue> {
        let mut m = crate::messages::Map::new();
        m.insert(
            name.to_string(),
            ChannelValue::vec3(x, 0.0, 0.0, Some(unit)),
        );
        m
    }

    #[test]
    fn nan_velocity_setpoint_fails_safe_to_hold() {
        let mut gov = SafetyGovernor::new(SafetyLimits {
            max_speed_mps: Some(5.0),
            ..Default::default()
        });
        let cmd = CommandFrame {
            channels: channels_with("velocity_setpoint", f64::NAN, "m/s"),
            ..Default::default()
        };
        // Fresh sensor (now == last → not stale); a NaN must not slip past the clamp.
        let out = gov.govern(&cmd, None, 1.0, Some(1.0));
        assert_eq!(out.mode, Mode::Hold, "NaN velocity must fail safe to HOLD");
        let v = out
            .channels
            .get("velocity_setpoint")
            .expect("setpoint present");
        assert!(v.data.iter().all(|c| *c == 0.0), "HOLD zeroes the setpoint");
    }

    #[test]
    fn nan_position_triggers_estop_under_active_geofence() {
        let mut gov = SafetyGovernor::new(SafetyLimits {
            geofence_radius_m: Some(10.0),
            ..Default::default()
        });
        let sensor = SensorFrame {
            channels: channels_with("pose_position", f64::NAN, "m"),
            ..Default::default()
        };
        let out = gov.govern(&CommandFrame::default(), Some(&sensor), 1.0, Some(1.0));
        assert_eq!(
            out.mode,
            Mode::Estop,
            "NaN position must fail safe to ESTOP under an active geofence"
        );
    }

    #[test]
    fn zero_geofence_radius_is_disabled() {
        let mut gov = SafetyGovernor::new(SafetyLimits {
            geofence_radius_m: Some(0.0),
            ..Default::default()
        });
        // pose 3,0,0 → r=3 > 0; with radius 0 the fence is disabled (matches loop.py).
        let sensor = SensorFrame {
            channels: channels_with("pose_position", 3.0, "m"),
            ..Default::default()
        };
        let out = gov.govern(&CommandFrame::default(), Some(&sensor), 1.0, Some(1.0));
        assert_eq!(
            out.mode,
            Mode::Active,
            "radius 0 disables the geofence; no ESTOP"
        );
    }

    // ───────────────────────── FIX 1: ESTOP latches until reset ─────────────────────────

    #[test]
    fn estop_latches_until_reset() {
        let mut gov = SafetyGovernor::new(SafetyLimits {
            geofence_radius_m: Some(10.0),
            ..Default::default()
        });
        // Breach the fence: pose 99,0,0 -> r=99 > 10 -> ESTOP.
        let breach = SensorFrame {
            channels: channels_with("pose_position", 99.0, "m"),
            ..Default::default()
        };
        let out = gov.govern(&CommandFrame::default(), Some(&breach), 1.0, Some(1.0));
        assert_eq!(out.mode, Mode::Estop, "geofence breach must ESTOP");
        assert!(gov.is_estopped());
        assert!(!gov.safety_ok());

        // Now feed a perfectly safe state — the latch must keep returning ESTOP.
        let inside = SensorFrame {
            channels: channels_with("pose_position", 1.0, "m"),
            ..Default::default()
        };
        let still = gov.govern(&CommandFrame::default(), Some(&inside), 2.0, Some(2.0));
        assert_eq!(
            still.mode,
            Mode::Estop,
            "ESTOP must latch — a safe sensor does NOT clear it"
        );
        let v = still
            .channels
            .get("velocity_setpoint")
            .expect("zeroed setpoint present");
        assert!(
            v.data.iter().all(|c| *c == 0.0),
            "latched ESTOP zeroes the command"
        );

        // Supervisor reset clears it; the next safe state is ACTIVE again.
        gov.reset();
        assert!(!gov.is_estopped());
        let after = gov.govern(&CommandFrame::default(), Some(&inside), 3.0, Some(3.0));
        assert_eq!(
            after.mode,
            Mode::Active,
            "after reset a safe state resumes ACTIVE"
        );
    }

    // ───────────────── FIX 2: default-deny on a bad command_timeout_ms ─────────────────

    #[test]
    fn bad_command_timeout_defaults_to_hold() {
        for bad in [f64::NAN, 0.0, -5.0, f64::NEG_INFINITY] {
            let mut gov = SafetyGovernor::new(SafetyLimits {
                command_timeout_ms: bad,
                ..Default::default()
            });
            // last == now: under a *valid* timeout this is the freshest possible
            // sensor (not stale). A bad timeout must STILL force HOLD (fail closed),
            // never fall through to ACTIVE.
            let out = gov.govern(&CommandFrame::default(), None, 10.0, Some(10.0));
            assert_eq!(
                out.mode,
                Mode::Hold,
                "timeout {bad} must fail closed to HOLD"
            );
        }
        // Sanity: a finite positive timeout with a fresh sensor is NOT stale.
        let mut ok = SafetyGovernor::new(SafetyLimits {
            command_timeout_ms: 500.0,
            ..Default::default()
        });
        assert_eq!(
            ok.govern(&CommandFrame::default(), None, 10.0, Some(10.0))
                .mode,
            Mode::Active
        );
    }

    // ───────────── FIX 3: geofence on a non-default channel; absent => fail closed ─────────────

    #[test]
    fn geofence_breach_on_non_default_channel_still_holds() {
        // Negotiated channel names differ from the historical literals.
        let mut gov = SafetyGovernor::with_channels(
            SafetyLimits {
                geofence_radius_m: Some(10.0),
                ..Default::default()
            },
            "ned_pos",                 // position channel (geofence input)
            "thrust_vec",              // velocity channel
            vec!["thrust_vec".into()], // negotiated command channels
            vec!["ned_pos".into()],    // negotiated sensor channels
        );
        // Breach on the *negotiated* channel name, not "pose_position".
        let breach = SensorFrame {
            channels: channels_with("ned_pos", 50.0, "m"),
            ..Default::default()
        };
        let out = gov.govern(&CommandFrame::default(), Some(&breach), 1.0, Some(1.0));
        assert_eq!(
            out.mode,
            Mode::Estop,
            "breach on the negotiated channel must ESTOP, not no-op"
        );
    }

    #[test]
    fn geofence_channel_absent_from_specs_fails_closed() {
        // A geofence is configured but its position channel is not in the negotiated
        // sensor specs -> misconfiguration -> fail closed (HOLD + safety_ok=false).
        let mut gov = SafetyGovernor::with_channels(
            SafetyLimits {
                geofence_radius_m: Some(10.0),
                ..Default::default()
            },
            "pose_position", // geofence wants this...
            "velocity_setpoint",
            vec!["velocity_setpoint".into()],
            vec!["imu_accel".into()], // ...but the sensor specs don't declare it
        );
        let sensor = SensorFrame {
            channels: channels_with("imu_accel", 0.0, "m/s2"),
            ..Default::default()
        };
        let out = gov.govern(&CommandFrame::default(), Some(&sensor), 1.0, Some(1.0));
        assert_eq!(
            out.mode,
            Mode::Hold,
            "undeclared geofence channel must fail closed to HOLD"
        );
        assert!(
            !gov.safety_ok(),
            "config fail-closed reports safety_ok=false"
        );
    }

    #[test]
    fn geofence_channel_missing_from_frame_holds() {
        // Channel IS declared in specs but the live sensor frame omits it -> cannot
        // evaluate the fence -> fail closed (HOLD), non-latching.
        let mut gov = SafetyGovernor::with_channels(
            SafetyLimits {
                geofence_radius_m: Some(10.0),
                ..Default::default()
            },
            "ned_pos",
            "thrust_vec",
            vec!["thrust_vec".into()],
            vec!["ned_pos".into()],
        );
        let sensor = SensorFrame {
            channels: channels_with("other", 1.0, "m"),
            ..Default::default()
        };
        let out = gov.govern(&CommandFrame::default(), Some(&sensor), 1.0, Some(1.0));
        assert_eq!(
            out.mode,
            Mode::Hold,
            "geofence channel missing from the frame -> HOLD"
        );
        assert!(
            !gov.is_estopped(),
            "a missing-frame channel HOLD must not latch"
        );
    }

    // ───────── CRITICAL: predictive horizon steps obey the same speed clamp ─────────

    #[test]
    fn horizon_steps_are_speed_clamped() {
        let mut gov = SafetyGovernor::new(SafetyLimits {
            max_speed_mps: Some(1.0),
            ..Default::default()
        });
        // tick 0 within limit; the horizon carries an over-limit step (mag 5).
        let mut tick0 = crate::messages::Map::new();
        tick0.insert(
            "velocity_setpoint".into(),
            ChannelValue::vec3(0.5, 0.0, 0.0, Some("m/s")),
        );
        let mut over = crate::messages::Map::new();
        over.insert(
            "velocity_setpoint".into(),
            ChannelValue::vec3(3.0, 4.0, 0.0, Some("m/s")), // mag 5 > 1
        );
        let cmd = CommandFrame {
            channels: tick0,
            horizon: vec![over],
            horizon_dt_ms: Some(50.0),
            ..Default::default()
        };
        // Fresh sensor, no geofence -> ACTIVE; the horizon must come back clamped.
        let out = gov.govern(&cmd, None, 1.0, Some(1.0));
        assert_eq!(out.mode, Mode::Active);
        let hv = &out.horizon[0]["velocity_setpoint"].data;
        let mag = hv.iter().map(|c| c * c).sum::<f64>().sqrt();
        assert!(
            (mag - 1.0).abs() < 1e-9,
            "horizon step must be clamped to max_speed (1.0), got {mag}"
        );
    }

    #[test]
    fn nonfinite_horizon_step_truncates_replay() {
        let mut gov = SafetyGovernor::new(SafetyLimits {
            max_speed_mps: Some(2.0),
            ..Default::default()
        });
        let step = |x: f64| {
            let mut m = crate::messages::Map::new();
            m.insert(
                "velocity_setpoint".into(),
                ChannelValue::vec3(x, 0.0, 0.0, Some("m/s")),
            );
            m
        };
        let cmd = CommandFrame {
            channels: step(0.5),
            // good, then non-finite, then good: replay must stop AT the poisoned step.
            horizon: vec![step(1.0), step(f64::NAN), step(1.0)],
            horizon_dt_ms: Some(50.0),
            ..Default::default()
        };
        let out = gov.govern(&cmd, None, 1.0, Some(1.0));
        assert_eq!(
            out.horizon.len(),
            1,
            "horizon truncates at the first unclampable (non-finite) step"
        );
    }

    #[test]
    fn nan_clock_forces_hold() {
        // safety-2: a non-finite `now_s` must be treated as stale (HOLD), not slip
        // past the `(now_s - last) > timeout` comparison as "fresh".
        let mut gov = SafetyGovernor::new(SafetyLimits::default());
        let out = gov.govern(&CommandFrame::default(), None, f64::NAN, Some(1.0));
        assert_eq!(out.mode, Mode::Hold, "NaN clock must fail safe to HOLD");

        let mut wd = CommandWatchdog::new();
        wd.on_command(1.0, 200.0, 1);
        assert!(wd.should_hold(f64::NAN), "watchdog HOLDs on a NaN clock");
    }

    #[test]
    fn hold_zeroes_all_command_channels() {
        // Inbound command carries two channels; negotiated command set adds a third.
        // A HOLD (here via stale sensor) must zero the UNION, not just one literal.
        let mut gov = SafetyGovernor::with_channels(
            SafetyLimits {
                command_timeout_ms: 500.0,
                ..Default::default()
            },
            "pose_position",
            "thrust_vec",
            vec!["thrust_vec".into(), "yaw_rate".into()],
            vec!["pose_position".into()],
        );
        let mut ch = crate::messages::Map::new();
        ch.insert(
            "thrust_vec".into(),
            ChannelValue::vec3(3.0, 4.0, 0.0, Some("m/s")),
        );
        ch.insert("aux_servo".into(), ChannelValue::scalar(7.0, Some("rad")));
        let cmd = CommandFrame {
            channels: ch,
            ..Default::default()
        };
        // No sensor -> stale -> HOLD.
        let out = gov.govern(&cmd, None, 1.0, None);
        assert_eq!(out.mode, Mode::Hold);
        for name in ["thrust_vec", "aux_servo", "yaw_rate"] {
            let cv = out
                .channels
                .get(name)
                .unwrap_or_else(|| panic!("{name} must be zeroed in HOLD"));
            assert!(
                cv.data.iter().all(|c| *c == 0.0),
                "{name} must be all zeros"
            );
        }
    }
}
