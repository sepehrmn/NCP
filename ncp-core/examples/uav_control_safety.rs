//! Full UAV motion + controls + safety exercise for NCP (read-only use of the lib).
//!
//! Demonstrates, end to end and deterministically:
//!   1. Closed-loop flight: NeuroControlLoop + ReflexController fly a simulated
//!      quad from an offset to the target (velocity setpoints, speed-clamped).
//!   2. SafetyGovernor gates: speed clamp, geofence -> latched ESTOP, stale-sensor
//!      HOLD, ESTOP-mode passthrough, non-finite clock fail-safe, horizon clamp.
//!   3. ActionBuffer predictive horizon replay through a dropout + ttl expiry.
//!   4. CommandWatchdog ttl deadline + out-of-order seq must not refresh it.
//!
//! Run: cargo run -p ncp-core --example uav_control_safety

use ncp_core::{
    ActionBuffer, ChannelValue, CommandFrame, CommandWatchdog, InProcessTransport, Map, Mode,
    NeuroControlLoop, ReflexController, SafetyGovernor, SafetyLimits, SensorFrame,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn vel_map(x: f64, y: f64, z: f64) -> Map<ChannelValue> {
    let mut m = Map::new();
    m.insert(
        "velocity_setpoint".into(),
        ChannelValue::vec3(x, y, z, Some("m/s")),
    );
    m
}
fn sensor_at(seq: i64, t: f64, p: [f64; 3], v: [f64; 3]) -> SensorFrame {
    let mut ch = Map::new();
    ch.insert(
        "pose_position".into(),
        ChannelValue::vec3(p[0], p[1], p[2], Some("m")),
    );
    ch.insert(
        "pose_velocity".into(),
        ChannelValue::vec3(v[0], v[1], v[2], Some("m/s")),
    );
    SensorFrame {
        seq,
        t,
        channels: ch,
        ..Default::default()
    }
}
fn vmag(c: &ChannelValue) -> f64 {
    c.data.iter().map(|x| x * x).sum::<f64>().sqrt()
}
fn vget(m: &Map<ChannelValue>) -> [f64; 3] {
    let c = m.get("velocity_setpoint").expect("velocity_setpoint");
    [c.data[0], c.data[1], c.data[2]]
}

struct T {
    pass: u32,
    fail: u32,
}
impl T {
    fn check(&mut self, name: &str, ok: bool, detail: String) {
        if ok {
            self.pass += 1;
            println!("  PASS  {name}  {detail}");
        } else {
            self.fail += 1;
            println!("  FAIL  {name}  {detail}");
        }
    }
}

fn main() {
    let mut t = T { pass: 0, fail: 0 };

    // ── 1. Closed-loop flight to target ─────────────────────────────────────
    println!("\n[1] Closed-loop PD flight (ReflexController via NeuroControlLoop)");
    let clock = Arc::new(AtomicU64::new(0)); // milliseconds
    let ck = clock.clone();
    let mut loop_ = NeuroControlLoop::new(
        InProcessTransport::new(),
        ReflexController {
            kp: 1.0,
            kd: 0.3,
            max_speed: 1.5,
            ..Default::default()
        },
        20.0,
        SafetyLimits {
            command_timeout_ms: 1000.0,
            ..Default::default()
        },
    )
    .with_clock(Box::new(move || ck.load(Ordering::Relaxed) as f64 / 1000.0));

    let dt = 0.05_f64; // 20 Hz
    let mut pos: [f64; 3] = [4.0, 3.0, -2.0];
    let mut vel: [f64; 3] = [0.0, 0.0, 0.0];
    let start_dist = (pos[0] * pos[0] + pos[1] * pos[1] + pos[2] * pos[2]).sqrt();
    let mut max_speed_seen = 0.0_f64; // vector magnitude
    let mut max_axis_seen = 0.0_f64; // per-component
    for k in 0..120 {
        clock.fetch_add((dt * 1000.0) as u64, Ordering::Relaxed);
        loop_
            .transport
            .push_sensor(sensor_at(k, k as f64 * dt, pos, vel));
        let cmd = loop_.tick();
        let v = vget(&cmd.channels);
        max_speed_seen = max_speed_seen.max((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt());
        max_axis_seen = max_axis_seen
            .max(v[0].abs())
            .max(v[1].abs())
            .max(v[2].abs());
        vel = v;
        for i in 0..3 {
            pos[i] += v[i] * dt;
        }
        if k % 30 == 0 {
            println!(
                "    t={:.2}s pos=({:5.2},{:5.2},{:5.2}) v=({:5.2},{:5.2},{:5.2})",
                k as f64 * dt,
                pos[0],
                pos[1],
                pos[2],
                v[0],
                v[1],
                v[2]
            );
        }
    }
    let end_dist = (pos[0] * pos[0] + pos[1] * pos[1] + pos[2] * pos[2]).sqrt();
    t.check(
        "flight converges to target",
        end_dist < 0.1,
        format!("dist {start_dist:.2}m -> {end_dist:.3}m"),
    );
    t.check(
        "ReflexController per-axis clamp (<=1.5)",
        max_axis_seen <= 1.5 + 1e-9,
        format!("peak |axis|={max_axis_seen:.3} m/s"),
    );
    println!("    NOTE: ReflexController clamps PER-AXIS, so vector speed reached {max_speed_seen:.3} m/s (up to sqrt(3)*max_speed); SafetyGovernor magnitude-clamps. [finding]");

    // ── 2. SafetyGovernor gates ─────────────────────────────────────────────
    println!("\n[2] SafetyGovernor gates");
    let fresh = sensor_at(1, 1.0, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);

    // 2a speed clamp
    let mut gov = SafetyGovernor::new(SafetyLimits {
        max_speed_mps: Some(5.0),
        command_timeout_ms: 1000.0,
        ..Default::default()
    });
    let cmd = CommandFrame {
        mode: Mode::Active,
        channels: vel_map(100.0, 0.0, 0.0),
        ..Default::default()
    };
    let out = gov.govern(&cmd, Some(&fresh), 1.0, Some(0.99));
    let mag = vmag(out.channels.get("velocity_setpoint").unwrap());
    t.check(
        "over-speed command clamped",
        (mag - 5.0).abs() < 1e-6,
        format!("|v| 100 -> {mag:.3} m/s"),
    );

    // 2b geofence breach -> latched estop
    let mut gov_g = SafetyGovernor::new(SafetyLimits {
        geofence_radius_m: Some(10.0),
        command_timeout_ms: 1000.0,
        ..Default::default()
    });
    let beyond = sensor_at(1, 1.0, [20.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
    let out = gov_g.govern(
        &CommandFrame {
            mode: Mode::Active,
            channels: vel_map(1.0, 0.0, 0.0),
            ..Default::default()
        },
        Some(&beyond),
        1.0,
        Some(0.99),
    );
    t.check(
        "geofence breach -> ESTOP latch",
        out.mode == Mode::Estop && gov_g.is_estopped(),
        format!("mode={:?} estopped={}", out.mode, gov_g.is_estopped()),
    );

    // 2c stale sensor -> HOLD (non-latching)
    let mut gov_s = SafetyGovernor::new(SafetyLimits {
        command_timeout_ms: 500.0,
        ..Default::default()
    });
    let out = gov_s.govern(&cmd, Some(&fresh), 10.0, Some(0.0));
    t.check(
        "stale sensor -> HOLD",
        out.mode == Mode::Hold && !gov_s.is_estopped(),
        format!("mode={:?}", out.mode),
    );

    // 2d ESTOP-mode command: govern preserves the mode for the plant to enforce.
    let mut gov_e = SafetyGovernor::new(SafetyLimits {
        command_timeout_ms: 1000.0,
        ..Default::default()
    });
    let out = gov_e.govern(
        &CommandFrame {
            mode: Mode::Estop,
            channels: vel_map(9.0, 9.0, 9.0),
            ..Default::default()
        },
        Some(&fresh),
        1.0,
        Some(0.99),
    );
    let z = vget(&out.channels);
    t.check(
        "ESTOP mode preserved for plant",
        out.mode == Mode::Estop,
        format!("mode={:?}", out.mode),
    );
    println!("    NOTE: govern() did NOT zero the channels of an incoming estop/hold-mode command (v={z:?}); de-energizing relies on the plant honoring `mode`. Defense-in-depth candidate. [finding]");

    // 2e non-finite clock -> fail-safe HOLD (not fail-open)
    let mut gov_n = SafetyGovernor::new(SafetyLimits {
        command_timeout_ms: 500.0,
        ..Default::default()
    });
    let out = gov_n.govern(&cmd, Some(&fresh), f64::NAN, Some(0.0));
    t.check(
        "NaN clock -> HOLD (fail-safe)",
        out.mode == Mode::Hold,
        format!("mode={:?}", out.mode),
    );

    // 2f horizon steps are clamped too
    let mut gov_h = SafetyGovernor::new(SafetyLimits {
        max_speed_mps: Some(5.0),
        command_timeout_ms: 1000.0,
        ..Default::default()
    });
    let cmd_h = CommandFrame {
        mode: Mode::Active,
        channels: vel_map(1.0, 0.0, 0.0),
        horizon: vec![vel_map(50.0, 0.0, 0.0)],
        horizon_dt_ms: Some(100.0),
        ..Default::default()
    };
    let out = gov_h.govern(&cmd_h, Some(&fresh), 1.0, Some(0.99));
    let hmag = out
        .horizon
        .first()
        .map(|m| vmag(m.get("velocity_setpoint").unwrap()))
        .unwrap_or(0.0);
    t.check(
        "predictive horizon clamped",
        (hmag - 5.0).abs() < 1e-6,
        format!("horizon |v| 50 -> {hmag:.3} m/s"),
    );

    // ── 3. ActionBuffer predictive replay through a dropout ──────────────────
    println!("\n[3] ActionBuffer horizon replay + ttl");
    let mut ab = ActionBuffer::new();
    let cmd_p = CommandFrame {
        mode: Mode::Active,
        ttl_ms: 500.0,
        channels: vel_map(1.0, 0.0, 0.0),
        horizon: vec![vel_map(2.0, 0.0, 0.0), vel_map(3.0, 0.0, 0.0)],
        horizon_dt_ms: Some(100.0),
        ..Default::default()
    };
    ab.on_command(0.0, cmd_p);
    let a0 = ab
        .active(0.0)
        .and_then(|m| m.get("velocity_setpoint").map(|c| c.data[0]));
    let a1 = ab
        .active(0.1)
        .and_then(|m| m.get("velocity_setpoint").map(|c| c.data[0]));
    let a2 = ab
        .active(0.2)
        .and_then(|m| m.get("velocity_setpoint").map(|c| c.data[0]));
    t.check(
        "replay tick0/h1/h2 through dropout",
        a0 == Some(1.0) && a1 == Some(2.0) && a2 == Some(3.0),
        format!("{a0:?} {a1:?} {a2:?}"),
    );
    t.check(
        "ttl expiry -> HOLD",
        ab.should_hold(0.6),
        format!("should_hold(0.6s)={}", ab.should_hold(0.6)),
    );

    // ── 4. CommandWatchdog deadline + out-of-order seq ──────────────────────
    println!("\n[4] CommandWatchdog");
    let mut wd = CommandWatchdog::new();
    wd.on_command(0.0, 200.0, 1);
    let within = wd.should_hold(0.1);
    let expired = wd.should_hold(0.3);
    t.check(
        "watchdog within ttl actuates",
        !within,
        format!("should_hold(0.1s)={within}"),
    );
    t.check(
        "watchdog past ttl HOLDs",
        expired,
        format!("should_hold(0.3s)={expired}"),
    );
    // refresh with a fresh command, then a stale (older seq) one must not refresh
    wd.on_command(0.25, 200.0, 2);
    wd.on_command(0.30, 200.0, 1); // older seq -> must be ignored
    let held = wd.should_hold(0.50); // 0.50 - 0.25 = 250ms > 200ms ttl
    t.check(
        "out-of-order seq doesn't refresh deadline",
        held,
        format!("should_hold(0.50s)={held}"),
    );

    println!(
        "\n=== UAV control + safety: {} passed, {} failed ===",
        t.pass, t.fail
    );
    std::process::exit(if t.fail == 0 { 0 } else { 1 });
}
