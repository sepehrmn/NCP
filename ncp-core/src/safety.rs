//! Safety governor for the **action plane** — the only plane with command
//! authority. Enforces the parts of `SafetyLimits` a controller can: HOLD on a
//! stale sensor, ESTOP on a geofence breach, and clamp speed. Returns a *fresh*
//! `CommandFrame` (never mutates the input). `max_tilt_rad` is advisory — the
//! plant / flight controller enforces it. Mirrors `loop.py::SafetyGovernor`.
//!
//! The watchdog (`govern` with `last_sensor_s = None` or stale) is the
//! producer-overrun backstop: if the brain (`nest.Run`) misses the deadline, the
//! plant-side governor fails safe to HOLD independent of the controller.

use crate::messages::{ChannelValue, CommandFrame, Mode, SafetyLimits, SensorFrame};

#[derive(Clone, Debug, Default)]
pub struct SafetyGovernor {
    pub limits: SafetyLimits,
}

impl SafetyGovernor {
    pub fn new(limits: SafetyLimits) -> Self {
        Self { limits }
    }

    /// Apply safety to `command`. `now_s` and `last_sensor_s` are wall-clock
    /// seconds; a missing/old sensor forces HOLD (fail-safe to zero, **not**
    /// latch-last).
    pub fn govern(
        &self,
        command: &CommandFrame,
        sensor: Option<&SensorFrame>,
        now_s: f64,
        last_sensor_s: Option<f64>,
    ) -> CommandFrame {
        let zero = || {
            let mut m = crate::messages::Map::new();
            m.insert("velocity_setpoint".to_string(), ChannelValue::vec3(0.0, 0.0, 0.0, Some("m/s")));
            m
        };

        let timeout = self.limits.command_timeout_ms.max(0.0) / 1000.0;
        let stale = match last_sensor_s {
            None => true,
            Some(last) => timeout > 0.0 && (now_s - last) > timeout,
        };
        if stale {
            return CommandFrame {
                t: command.t,
                seq: command.seq,
                mode: Mode::Hold,
                channels: zero(),
                ..Default::default()
            };
        }

        if let (Some(radius), Some(sensor)) = (self.limits.geofence_radius_m, sensor) {
            if let Some(pos) = sensor.channels.get("pose_position") {
                let r = pos.data.iter().map(|c| c * c).sum::<f64>().sqrt();
                // `radius > 0.0` disables a zero/negative fence (matches loop.py);
                // a non-finite `r` (NaN from upstream) fails safe to ESTOP rather
                // than silently passing the `r > radius` comparison.
                if radius > 0.0 && (!r.is_finite() || r > radius) {
                    return CommandFrame {
                        t: command.t,
                        seq: command.seq,
                        mode: Mode::Estop,
                        channels: zero(),
                        ..Default::default()
                    };
                }
            }
        }

        let mut out = command.clone();
        if let (Some(max_speed), Some(vel)) =
            (self.limits.max_speed_mps, out.channels.get("velocity_setpoint").cloned())
        {
            let mag = vel.data.iter().map(|c| c * c).sum::<f64>().sqrt();
            if !mag.is_finite() {
                // A non-finite command (e.g. divide-by-zero upstream) would slip
                // past the `mag > max_speed` comparison unclamped — fail safe to HOLD.
                return CommandFrame {
                    t: command.t,
                    seq: command.seq,
                    mode: Mode::Hold,
                    channels: zero(),
                    ..Default::default()
                };
            }
            if max_speed > 0.0 && mag > max_speed {
                let k = max_speed / mag;
                out.channels.insert(
                    "velocity_setpoint".to_string(),
                    ChannelValue { data: vel.data.iter().map(|c| c * k).collect(), unit: vel.unit },
                );
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
        m.insert(name.to_string(), ChannelValue::vec3(x, 0.0, 0.0, Some(unit)));
        m
    }

    #[test]
    fn nan_velocity_setpoint_fails_safe_to_hold() {
        let gov = SafetyGovernor::new(SafetyLimits { max_speed_mps: Some(5.0), ..Default::default() });
        let cmd = CommandFrame { channels: channels_with("velocity_setpoint", f64::NAN, "m/s"), ..Default::default() };
        // Fresh sensor (now == last → not stale); a NaN must not slip past the clamp.
        let out = gov.govern(&cmd, None, 1.0, Some(1.0));
        assert_eq!(out.mode, Mode::Hold, "NaN velocity must fail safe to HOLD");
        let v = out.channels.get("velocity_setpoint").expect("setpoint present");
        assert!(v.data.iter().all(|c| *c == 0.0), "HOLD zeroes the setpoint");
    }

    #[test]
    fn nan_position_triggers_estop_under_active_geofence() {
        let gov = SafetyGovernor::new(SafetyLimits { geofence_radius_m: Some(10.0), ..Default::default() });
        let sensor = SensorFrame { channels: channels_with("pose_position", f64::NAN, "m"), ..Default::default() };
        let out = gov.govern(&CommandFrame::default(), Some(&sensor), 1.0, Some(1.0));
        assert_eq!(out.mode, Mode::Estop, "NaN position must fail safe to ESTOP under an active geofence");
    }

    #[test]
    fn zero_geofence_radius_is_disabled() {
        let gov = SafetyGovernor::new(SafetyLimits { geofence_radius_m: Some(0.0), ..Default::default() });
        // pose 3,0,0 → r=3 > 0; with radius 0 the fence is disabled (matches loop.py).
        let sensor = SensorFrame { channels: channels_with("pose_position", 3.0, "m"), ..Default::default() };
        let out = gov.govern(&CommandFrame::default(), Some(&sensor), 1.0, Some(1.0));
        assert_eq!(out.mode, Mode::Active, "radius 0 disables the geofence; no ESTOP");
    }
}
