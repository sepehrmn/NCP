//! NCP overhead measurement — "is NCP low overhead?" (read-only use of the lib).
//!
//! Times the per-tick hot paths: JSON (de)serialization of the action/perception
//! frames, the safety governor, the reflex controller, and the binary BulkBlock
//! observation codec (vs JSON for the same payload).
//!
//! Run: cargo run -p ncp-core --release --example overhead

use ncp_core::transport::Controller;
use ncp_core::{
    BulkBlock, ChannelValue, Column, CommandFrame, Map, Mode, ReflexController, SafetyGovernor,
    SafetyLimits, SensorFrame,
};
use std::hint::black_box;
use std::time::Instant;

fn bench<F: FnMut()>(iters: u64, mut f: F) -> f64 {
    for _ in 0..(iters / 20).max(1) {
        f();
    } // warm
    let t = Instant::now();
    for _ in 0..iters {
        f();
    }
    t.elapsed().as_nanos() as f64 / iters as f64
}
fn row(name: &str, ns: f64, bytes: usize) {
    let per_sec = if ns > 0.0 { 1e9 / ns } else { 0.0 };
    let b = if bytes > 0 {
        format!("{bytes} B")
    } else {
        "-".into()
    };
    println!("  {name:38} {ns:9.1} ns/op   {per_sec:>11.0} ops/s   {b}");
}

fn main() {
    // typical action frame
    let mut ch = Map::new();
    ch.insert(
        "velocity_setpoint".into(),
        ChannelValue::vec3(0.4, -0.1, 0.2, Some("m/s")),
    );
    let cmd = CommandFrame {
        mode: Mode::Active,
        ttl_ms: 200.0,
        seq: 42,
        channels: ch,
        ..Default::default()
    };
    let cmd_bytes = serde_json::to_vec(&cmd).unwrap();

    // typical sensor frame
    let mut sch = Map::new();
    sch.insert(
        "pose_position".into(),
        ChannelValue::vec3(1.0, 2.0, 3.0, Some("m")),
    );
    sch.insert(
        "pose_velocity".into(),
        ChannelValue::vec3(0.1, 0.0, -0.2, Some("m/s")),
    );
    let sensor = SensorFrame {
        seq: 42,
        t: 1.0,
        channels: sch,
        ..Default::default()
    };
    let sensor_bytes = serde_json::to_vec(&sensor).unwrap();

    println!("\n=== NCP per-tick hot-path overhead (JSON action/perception planes) ===");
    let n = 500_000;
    row(
        "CommandFrame serialize (serde_json)",
        bench(n, || {
            black_box(serde_json::to_vec(black_box(&cmd)).unwrap());
        }),
        cmd_bytes.len(),
    );
    row(
        "CommandFrame deserialize",
        bench(n, || {
            let c: CommandFrame = serde_json::from_slice(black_box(&cmd_bytes)).unwrap();
            black_box(c);
        }),
        cmd_bytes.len(),
    );
    row(
        "SensorFrame serialize",
        bench(n, || {
            black_box(serde_json::to_vec(black_box(&sensor)).unwrap());
        }),
        sensor_bytes.len(),
    );
    row(
        "SensorFrame deserialize",
        bench(n, || {
            let s: SensorFrame = serde_json::from_slice(black_box(&sensor_bytes)).unwrap();
            black_box(s);
        }),
        sensor_bytes.len(),
    );

    println!("\n=== control + safety compute ===");
    let mut gov = SafetyGovernor::new(SafetyLimits {
        max_speed_mps: Some(5.0),
        geofence_radius_m: Some(100.0),
        command_timeout_ms: 1000.0,
        ..Default::default()
    });
    row(
        "SafetyGovernor.govern",
        bench(n, || {
            black_box(gov.govern(black_box(&cmd), Some(black_box(&sensor)), 1.0, Some(0.99)));
        }),
        0,
    );
    let mut ctrl = ReflexController::default();
    row(
        "ReflexController.step",
        bench(n, || {
            black_box(ctrl.step(Some(black_box(&sensor)), 50.0));
        }),
        0,
    );

    println!("\n=== bulk observation codec: binary BulkBlock vs JSON (1000 spike times) ===");
    let times: Vec<f64> = (0..1000).map(|i| i as f64 * 0.137).collect();
    let block = BulkBlock::new().with("times", Column::F64(times.clone()));
    let bulk_bytes = block.encode();
    let json_bytes = serde_json::to_vec(&times).unwrap();
    row(
        "BulkBlock encode (1000 f64)",
        bench(50_000, || {
            black_box(
                BulkBlock::new()
                    .with("times", Column::F64(black_box(times.clone())))
                    .encode(),
            );
        }),
        bulk_bytes.len(),
    );
    row(
        "BulkBlock decode",
        bench(50_000, || {
            black_box(BulkBlock::decode(black_box(&bulk_bytes)).unwrap());
        }),
        bulk_bytes.len(),
    );
    row(
        "(JSON encode same 1000 f64)",
        bench(50_000, || {
            black_box(serde_json::to_vec(black_box(&times)).unwrap());
        }),
        json_bytes.len(),
    );

    println!("\n=== verdict ===");
    println!(
        "  action frame: {} B JSON, ser+de ~{:.0}+{:.0} ns",
        cmd_bytes.len(),
        bench(n, || {
            black_box(serde_json::to_vec(&cmd).unwrap());
        }),
        bench(n, || {
            let c: CommandFrame = serde_json::from_slice(&cmd_bytes).unwrap();
            black_box(c);
        })
    );
    println!(
        "  bulk codec: BulkBlock {} B vs JSON {} B ({:.1}x smaller) for 1000 floats",
        bulk_bytes.len(),
        json_bytes.len(),
        json_bytes.len() as f64 / bulk_bytes.len() as f64
    );
}
