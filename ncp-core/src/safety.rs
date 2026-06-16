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
                if r > radius {
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
