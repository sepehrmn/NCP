//! Safety governor for the **action plane** — the only plane with command
//! authority. Enforces the parts of `SafetyLimits` a controller can: HOLD on a
//! stale sensor, **latch** ESTOP on a geofence breach, and clamp speed. Returns a
//! *fresh* `CommandFrame` (never mutates the input). `max_tilt_rad` is advisory —
//! the plant / flight controller enforces it. Mirrors `loop.py::SafetyGovernor`.
//!
//! ESTOP **latches**: once any condition trips it, every subsequent `govern`
//! returns ESTOP + a zeroed command until a supervisor calls [`reset`]. HOLD (on a
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
    /// by [`reset`]. While set, every `govern` returns a zeroed ESTOP frame.
    estop: bool,
    /// Latched config-level fail-closed: a limit (geofence/speed) was set whose
    /// channel is absent from the negotiated specs. Per FIX 3 the governor then
    /// HOLDs and reports `safety_ok=false`. A misconfiguration cannot be fixed at
    /// runtime, so it does not clear on [`reset`].
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
    /// Prefer [`from_capabilities`] so the enforced channels track the negotiated
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
    /// zeroed ESTOP until [`reset`]. Takes `&mut self` because of that latch.
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
                !timeout_ms.is_finite()
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
                let vel = out.channels.get(&self.velocity_channel).cloned();
                match vel {
                    None => {
                        // The speed limit references a channel that is not present on
                        // this command -> cannot enforce -> fail closed (HOLD).
                        return self.hold_frame(command);
                    }
                    Some(vel) => {
                        let mag = vel.data.iter().map(|c| c * c).sum::<f64>().sqrt();
                        if !mag.is_finite() {
                            // A non-finite command (divide-by-zero upstream) would slip
                            // past the `mag > max_speed` comparison — fail safe to HOLD.
                            return self.hold_frame(command);
                        }
                        if mag > max_speed {
                            let k = max_speed / mag;
                            out.channels.insert(
                                self.velocity_channel.clone(),
                                ChannelValue {
                                    data: vel.data.iter().map(|c| c * k).collect(),
                                    unit: vel.unit,
                                },
                            );
                        }
                    }
                }
            }
        }
        out
    }
}

/// Plant-side deadline backstop that **enforces `CommandFrame.ttl_ms`** — which is
/// otherwise carried on the wire but never checked. Feed each accepted command's
/// **local** arrival time and its `ttl_ms`; the plant must fail safe (HOLD to a
/// zero/safe setpoint) once the latest command has expired or none has arrived.
/// Using the plant's own clock avoids controller↔plant clock skew. This is the
/// deadline backstop the packetized-predictive-control horizon (see RESILIENCE.md)
/// relies on: replay buffered predictions only while unexpired, HOLD on drain.
#[derive(Clone, Debug, Default)]
pub struct CommandWatchdog {
    last_recv_s: Option<f64>,
    ttl_s: f64,
}

impl CommandWatchdog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an accepted command received at local time `now_s` with its `ttl_ms`.
    pub fn on_command(&mut self, now_s: f64, ttl_ms: f64) {
        self.last_recv_s = Some(now_s);
        self.ttl_s = ttl_ms.max(0.0) / 1000.0;
    }

    /// True if the plant must fail safe to HOLD: no command yet, or the latest is
    /// past its ttl. (A non-positive ttl is treated as immediately stale.)
    pub fn should_hold(&self, now_s: f64) -> bool {
        match self.last_recv_s {
            None => true,
            Some(t) => self.ttl_s <= 0.0 || (now_s - t) > self.ttl_s,
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
        wd.on_command(1.0, 200.0); // ttl 200 ms
        assert!(!wd.should_hold(1.1), "within ttl -> apply");
        assert!(wd.should_hold(1.3), "0.3 s > 0.2 s ttl -> HOLD");
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
