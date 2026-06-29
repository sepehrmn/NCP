//! UAV "drone-in-the-loop" driven entirely by NCP CommandFrames over Zenoh.
//!
//! This exercises the **action plane** end-to-end, true to wire v0.5:
//!   controller  --CommandFrame{velocity_setpoint, mode, ttl_ms, seq}-->  plant
//! on the exact key CREBAIN's bridge subscribes to
//! (`engram/ncp/session/<id>/command`, realm overridable via NCP_REALM).
//!
//! The plant is a minimal quad model: it integrates the commanded velocity into a
//! position, and it ENFORCES the protocol's two safety gates locally: `mode` in
//! {hold, estop} de-energizes (zero velocity), and the `ttl_ms` watchdog fails
//! safe to HOLD when no fresh command arrives — so a dropout or an e-stop visibly
//! stops the drone.
//!
//! It prints the trajectory and also appends it as JSONL to
//! $NCP_TRAJ_OUT (default ./ncp_drone_trajectory.jsonl) so an external consumer
//! (e.g. the CREBAIN browser drone, replayed via Playwright) can fly the same path.
//!
//! Run:  cargo run -p ncp-zenoh --example uav_drone_loop
//! Single in-process Zenoh session (scouting off) — no router required.

use ncp_core::keys::Keys;
use ncp_core::{ChannelValue, CommandFrame, Mode};
use ncp_zenoh::{ZenohBus, ZenohConfig};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default)]
struct DroneState {
    x: f64,
    y: f64,
    z: f64,
}

/// Latest action-plane command + when it arrived (for the ttl watchdog).
#[derive(Clone)]
struct LatestCmd {
    frame: CommandFrame,
    arrived: Instant,
}

fn vel(cmd: &CommandFrame) -> [f64; 3] {
    match cmd.channels.get("velocity_setpoint") {
        Some(c) if c.data.len() >= 3 => [c.data[0], c.data[1], c.data[2]],
        _ => [0.0, 0.0, 0.0],
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let realm = std::env::var("NCP_REALM").unwrap_or_else(|_| "engram/ncp".to_string());
    let session = "uav1";
    let traj_path =
        std::env::var("NCP_TRAJ_OUT").unwrap_or_else(|_| "ncp_drone_trajectory.jsonl".to_string());

    // One in-process Zenoh session; loopback delivery (no external discovery).
    let mut cfg = ZenohConfig::default();
    cfg.insert_json5("scouting/multicast/enabled", "false")
        .unwrap();
    cfg.insert_json5("scouting/gossip/enabled", "false")
        .unwrap();
    let bus = ZenohBus::with_config(cfg, Keys::new(realm.clone()))
        .await
        .expect("open zenoh bus");
    println!("NCP UAV loop  realm={realm}  key=ncp/session/{session}/command  -> {traj_path}");

    // ---- PLANT: subscribe to the action plane -------------------------------
    let latest: Arc<Mutex<Option<LatestCmd>>> = Arc::new(Mutex::new(None));
    let sink = latest.clone();
    bus.subscribe_commands(session, move |_k, bytes| {
        if let Ok(frame) = serde_json::from_slice::<CommandFrame>(&bytes) {
            *sink.lock().unwrap() = Some(LatestCmd {
                frame,
                arrived: Instant::now(),
            });
        }
    })
    .await
    .expect("subscribe commands");

    // Let the subscription declaration settle before the controller publishes.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // ---- PLANT integrator: 50 Hz, applies safety gates ----------------------
    let traj = Arc::new(Mutex::new(Vec::<String>::new()));
    let plant_traj = traj.clone();
    let plant_latest = latest.clone();
    let plant = tokio::spawn(async move {
        let dt = 0.02_f64; // 50 Hz
        let mut s = DroneState::default();
        let mut t = 0.0_f64;
        let steps = (8.0 / dt) as usize; // 8 s
        let mut tick = Duration::from_millis(20);
        loop {
            tokio::time::sleep(tick).await;
            t += dt;

            // Resolve the active command under the protocol's safety gates.
            let (mode, mut v) = match plant_latest.lock().unwrap().clone() {
                Some(c) => {
                    let age_ms = c.arrived.elapsed().as_secs_f64() * 1000.0;
                    if age_ms > c.frame.ttl_ms {
                        ("WATCHDOG_HOLD", [0.0, 0.0, 0.0]) // ttl expired -> fail-safe
                    } else {
                        match c.frame.mode {
                            Mode::Active => ("active", vel(&c.frame)),
                            Mode::Init => ("init", [0.0, 0.0, 0.0]),
                            Mode::Hold => ("hold", [0.0, 0.0, 0.0]),
                            Mode::Estop => ("estop", [0.0, 0.0, 0.0]),
                        }
                    }
                }
                None => ("no_command", [0.0, 0.0, 0.0]),
            };
            // ground constraint: don't sink below 0
            if s.y <= 0.0 && v[1] < 0.0 {
                v[1] = 0.0;
            }
            s.x += v[0] * dt;
            s.y += v[1] * dt;
            s.z += v[2] * dt;
            if s.y < 0.0 {
                s.y = 0.0;
            }

            let line = format!(
                "{{\"t\":{:.2},\"x\":{:.3},\"y\":{:.3},\"z\":{:.3},\"vx\":{:.3},\"vy\":{:.3},\"vz\":{:.3},\"mode\":\"{}\"}}",
                t, s.x, s.y, s.z, v[0], v[1], v[2], mode
            );
            plant_traj.lock().unwrap().push(line);
            if (t * 50.0) as usize % 10 == 0 {
                // ~5 Hz console print
                println!(
                    "  t={:4.2}s  pos=({:6.2},{:6.2},{:6.2})  v=({:5.2},{:5.2},{:5.2})  [{}]",
                    t, s.x, s.y, s.z, v[0], v[1], v[2], mode
                );
            }
            if t >= 8.0 - dt || ((t * 50.0) as usize) >= steps {
                break;
            }
        }
        // ensure unused var warning-free
        let _ = &mut tick;
    });

    // ---- CONTROLLER: publish a flight plan as CommandFrames -----------------
    // Plan (8 s): climb -> forward -> right -> HOLD (mode) -> dropout (ttl) -> ESTOP.
    let plan: &[(f64, &str, [f64; 3])] = &[
        (1.5, "active", [0.0, 1.5, 0.0]),  // climb to ~2.25 m
        (1.5, "active", [2.0, 0.0, 0.0]),  // fly +x
        (1.5, "active", [0.0, 0.0, 2.0]),  // fly +z
        (1.0, "hold", [0.0, 0.0, 0.0]),    // explicit HOLD (mode gate)
        (1.0, "dropout", [0.0, 0.0, 0.0]), // stop publishing -> ttl watchdog HOLD
        (1.0, "estop", [9.0, 9.0, 9.0]),   // ESTOP: nonzero vel MUST be ignored
    ];
    let hz = 20.0;
    let period = Duration::from_secs_f64(1.0 / hz);
    let mut seq: i64 = 0;
    let mut t = 0.0_f64;
    for (dur, mode_s, v) in plan {
        let n = (dur * hz) as usize;
        for _ in 0..n {
            t += 1.0 / hz;
            if *mode_s == "dropout" {
                // simulate a control dropout: publish nothing; plant must hold via ttl
                tokio::time::sleep(period).await;
                continue;
            }
            seq += 1;
            let mode = match *mode_s {
                "active" => Mode::Active,
                "hold" => Mode::Hold,
                "estop" => Mode::Estop,
                _ => Mode::Hold,
            };
            let mut channels = ncp_core::Map::new();
            channels.insert(
                "velocity_setpoint".to_string(),
                ChannelValue::vec3(v[0], v[1], v[2], Some("m/s")),
            );
            let cmd = CommandFrame {
                seq,
                t: t * 1000.0,
                ttl_ms: 250.0, // ~5 missed frames at 20 Hz
                mode,
                channels,
                ..Default::default()
            };
            let payload = serde_json::to_vec(&cmd).unwrap();
            bus.publish_command(session, &payload).await.unwrap();
            tokio::time::sleep(period).await;
        }
    }

    plant.await.unwrap();

    // Persist the trajectory for external replay (CREBAIN browser drone).
    let lines = traj.lock().unwrap().clone();
    if let Ok(mut f) = std::fs::File::create(&traj_path) {
        for l in &lines {
            let _ = writeln!(f, "{l}");
        }
        println!("wrote {} trajectory samples to {}", lines.len(), traj_path);
    }
    let _ = bus.close().await;
    println!("done.");
}
