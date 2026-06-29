//! The **safety governor over a real Zenoh transport** (RELEASE_READINESS blocker #1).
//!
//! `cross_session_rpc.rs` proves the control-plane *lifecycle* crosses two
//! independent Zenoh sessions. This proves the **action plane's safety authority**
//! does too: that `SafetyGovernor::govern` — the HOLD / latched-ESTOP / speed-clamp
//! gate that is the only thing standing between a controller and an actuator —
//! produces the same verdict when its `CommandFrame` and `SensorFrame` travel over
//! the wire as it does in-process, and (the property a unit test cannot show) that
//! an **ESTOP latch survives the transport**: once a geofence breach trips it, a
//! subsequent perfectly-safe frame is *still* ESTOP across the link.
//!
//! Topology (two independent sessions over a real localhost tcp link, multicast
//! discovery off — the two-process deployment path):
//!   - the **plant** session LISTENs; it subscribes to the perception plane
//!     (`…/sensor`) and the action plane (`…/command`), runs the governor on each
//!     received command against its latest sensor, and publishes the *governed*
//!     command back on the reliable observation plane so the test can read it.
//!   - the **controller** session CONNECTs; it publishes sensors + commands and
//!     subscribes to the governed read-back.
//!
//! The governor's limits and clock are **plant-side deployment config** (a real
//! plant reads its own `SafetyLimits`, not the wire), so they are set in-process;
//! the `CommandFrame`, the `SensorFrame`, and the governed result all cross the
//! real Zenoh transport — which is exactly what this test exists to prove.
//!
//! Expectations are driven from `conformance/behavior/vectors.json` (the same
//! `govern` cases the in-process `behavior_conformance.rs` checks) so the wire test
//! and the language-neutral corpus cannot diverge. Each request is stamped with a
//! unique `seq` (which the governor preserves) so a straggler frame on the lossy
//! action/perception planes can never be mistaken for the current case's verdict.

use ncp_core::keys::Keys;
use ncp_core::{CommandFrame, SafetyGovernor, SafetyLimits, SensorFrame};
use ncp_zenoh::{ZenohBus, ZenohConfig};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SID: &str = "uav-gov";
static SEQ: AtomicI64 = AtomicI64::new(1);

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn base_cfg() -> ZenohConfig {
    let mut c = ZenohConfig::default();
    c.insert_json5("scouting/multicast/enabled", "false")
        .unwrap();
    c.insert_json5("scouting/gossip/enabled", "false").unwrap();
    c
}

fn listen_cfg(port: u16) -> ZenohConfig {
    let mut c = base_cfg();
    c.insert_json5("listen/endpoints", &format!("[\"tcp/127.0.0.1:{port}\"]"))
        .unwrap();
    c
}

fn connect_cfg(port: u16) -> ZenohConfig {
    let mut c = base_cfg();
    c.insert_json5("connect/endpoints", &format!("[\"tcp/127.0.0.1:{port}\"]"))
        .unwrap();
    c
}

/// Load the `govern` cases from the shared behavioral corpus (same path the
/// in-process conformance test uses; `conformance/` travels with the workspace).
fn govern_cases() -> Vec<Value> {
    let path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../conformance/behavior"
    ))
    .join("vectors.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read behavior corpus {}: {e}", path.display()));
    let corpus: Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("corpus is not valid JSON: {e}"));
    corpus["cases"]["govern"]
        .as_array()
        .expect("corpus has no `govern` cases")
        .clone()
}

/// L2 magnitude of the governed command's `velocity_setpoint` channel — identical
/// to `behavior_conformance.rs::velocity_magnitude` so the wire test scores the
/// corpus exactly as the in-process reference does.
fn velocity_magnitude(frame: &Value) -> f64 {
    frame["channels"]["velocity_setpoint"]["data"]
        .as_array()
        .map(|data| {
            data.iter()
                .filter_map(Value::as_f64)
                .map(|c| c * c)
                .sum::<f64>()
                .sqrt()
        })
        .unwrap_or(0.0)
}

/// The plant's governor + clock + latest sensor, shared between the Zenoh callbacks
/// and the test driver (one process hosts both sessions). The driver configures the
/// governor per case; the callbacks read it.
struct PlantState {
    gov: SafetyGovernor,
    now_s: f64,
    last_sensor_s: Option<f64>,
    latest_sensor: Option<SensorFrame>,
}

/// Stand up the plant: subscribe to the perception + action planes; on each command
/// run the governor against the latest sensor and publish the governed command on
/// the reliable observation plane for read-back.
async fn spawn_plant(server: &ZenohBus, state: Arc<Mutex<PlantState>>) {
    {
        let st = state.clone();
        server
            .subscribe_sensors(SID, move |_k, bytes| {
                if let Ok(sf) = serde_json::from_slice::<SensorFrame>(&bytes) {
                    st.lock().unwrap().latest_sensor = Some(sf);
                }
            })
            .await
            .expect("subscribe sensors");
    }
    let st = state.clone();
    let pub_bus = server.clone();
    let handle = tokio::runtime::Handle::current();
    server
        .subscribe_commands(SID, move |_k, bytes| {
            let Ok(command) = serde_json::from_slice::<CommandFrame>(&bytes) else {
                return;
            };
            // Govern under the lock (govern takes &mut self for the ESTOP latch),
            // then release before the async publish.
            let governed = {
                let mut s = st.lock().unwrap();
                let sensor = s.latest_sensor.clone();
                let now = s.now_s;
                let last = s.last_sensor_s;
                s.gov.govern(&command, sensor.as_ref(), now, last)
            };
            let Ok(out) = serde_json::to_vec(&governed) else {
                return;
            };
            let bus = pub_bus.clone();
            handle.spawn(async move {
                let _ = bus.publish_observation(SID, &out).await;
            });
        })
        .await
        .expect("subscribe commands");
}

/// Drive one governed exchange over the wire: stamp `command`/`sensor` with a fresh
/// `seq`, publish the sensor (perception plane) and wait until the plant has stored
/// *this* sensor, then publish the command (action plane) and return the governed
/// frame the plant emits with the matching `seq`. The `seq` match makes the test
/// immune to dropped/duplicated frames on the lossy planes.
async fn govern_over_wire(
    client: &ZenohBus,
    state: &Arc<Mutex<PlantState>>,
    sink: &Arc<Mutex<Vec<Value>>>,
    mut command: CommandFrame,
    mut sensor: SensorFrame,
) -> Value {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    command.seq = seq;
    sensor.seq = seq;
    let sbytes = serde_json::to_vec(&sensor).unwrap();
    let cbytes = serde_json::to_vec(&command).unwrap();

    // Publish the sensor until the plant has stored exactly this one (seq match):
    // guarantees the correct sensor is in place before the command is governed,
    // regardless of cross-key delivery ordering.
    let mut sensor_ready = false;
    for _ in 0..100 {
        client.put_sensor(SID, &sbytes).await.expect("put sensor");
        tokio::time::sleep(Duration::from_millis(50)).await;
        if state
            .lock()
            .unwrap()
            .latest_sensor
            .as_ref()
            .is_some_and(|s| s.seq == seq)
        {
            sensor_ready = true;
            break;
        }
    }
    assert!(
        sensor_ready,
        "sensor (seq {seq}) never crossed the perception plane within ~5s"
    );

    // Publish the command and wait for the governed read-back with the same seq.
    for _ in 0..100 {
        client
            .publish_command(SID, &cbytes)
            .await
            .expect("publish command");
        tokio::time::sleep(Duration::from_millis(50)).await;
        if let Some(v) = sink
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|v| v["seq"].as_i64() == Some(seq))
            .cloned()
        {
            return v;
        }
    }
    panic!("governed command (seq {seq}) never came back over the wire within ~5s");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn safety_governor_decisions_survive_the_wire() {
    let port = free_port();

    let server = ZenohBus::with_config(listen_cfg(port), Keys::default())
        .await
        .expect("open plant session (listen)");
    let state = Arc::new(Mutex::new(PlantState {
        gov: SafetyGovernor::new(SafetyLimits::default()),
        now_s: 0.0,
        last_sensor_s: None,
        latest_sensor: None,
    }));
    spawn_plant(&server, state.clone()).await;

    let client = ZenohBus::with_config(connect_cfg(port), Keys::default())
        .await
        .expect("open controller session (connect)");
    let sink: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let snk = sink.clone();
        client
            .subscribe_observations(SID, move |_k, bytes| {
                if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                    snk.lock().unwrap().push(v);
                }
            })
            .await
            .expect("subscribe observations");
    }

    // ── 1. Every `govern` corpus case must reach the same verdict over the wire ──
    // Each case gets a FRESH governor (govern() latches ESTOP) configured from the
    // case's limits/clock; the command + sensor cross the real transport.
    for case in govern_cases() {
        let name = case["name"].as_str().unwrap().to_string();
        let input = &case["input"];
        let limits: SafetyLimits = serde_json::from_value(input["limits"].clone())
            .unwrap_or_else(|e| panic!("govern[{name}]: bad limits: {e}"));
        let command: CommandFrame = serde_json::from_value(input["command"].clone())
            .unwrap_or_else(|e| panic!("govern[{name}]: bad command: {e}"));
        let sensor: SensorFrame = serde_json::from_value(input["sensor"].clone())
            .unwrap_or_else(|e| panic!("govern[{name}]: bad sensor: {e}"));
        let now_s = input["now_s"].as_f64().expect("now_s");
        let last_sensor_s = input["last_sensor_s"].as_f64(); // None when null
        {
            let mut s = state.lock().unwrap();
            s.gov = SafetyGovernor::new(limits);
            s.now_s = now_s;
            s.last_sensor_s = last_sensor_s;
        }
        let got = govern_over_wire(&client, &state, &sink, command, sensor).await;
        assert_eq!(
            got["mode"].as_str().unwrap(),
            case["expect"]["mode"].as_str().unwrap(),
            "govern[{name}]: mode over the wire"
        );
        if let Some(want) = case["expect"]["velocity_setpoint_magnitude"].as_f64() {
            let got_mag = velocity_magnitude(&got);
            assert!(
                (got_mag - want).abs() < 1e-9,
                "govern[{name}]: velocity magnitude want {want}, got {got_mag} over the wire"
            );
        }
    }

    // ── 2. The ESTOP LATCH survives the transport (the wire-specific property) ──
    // One persistent governor: a geofence breach latches ESTOP, and a SUBSEQUENT
    // perfectly-safe frame — a full second round trip later — is STILL ESTOP.
    {
        let mut s = state.lock().unwrap();
        s.gov = SafetyGovernor::new(SafetyLimits {
            geofence_radius_m: Some(5.0),
            command_timeout_ms: 500.0,
            ..Default::default()
        });
        s.now_s = 1.0;
        s.last_sensor_s = Some(1.0);
    }
    let active_cmd: CommandFrame = serde_json::from_value(json!({
        "kind": "command_frame", "mode": "active",
        "channels": {"velocity_setpoint": {"data": [2.0, 0.0, 0.0], "unit": "m/s"}}
    }))
    .unwrap();
    let breach: SensorFrame = serde_json::from_value(json!({
        "kind": "sensor_frame", "channels": {"pose_position": {"data": [10.0, 0.0, 0.0]}}
    }))
    .unwrap();
    let estopped = govern_over_wire(&client, &state, &sink, active_cmd.clone(), breach).await;
    assert_eq!(
        estopped["mode"].as_str().unwrap(),
        "estop",
        "a geofence breach must ESTOP over the wire"
    );

    let safe: SensorFrame = serde_json::from_value(json!({
        "kind": "sensor_frame", "channels": {"pose_position": {"data": [0.0, 0.0, 0.0]}}
    }))
    .unwrap();
    let still = govern_over_wire(&client, &state, &sink, active_cmd, safe).await;
    assert_eq!(
        still["mode"].as_str().unwrap(),
        "estop",
        "ESTOP must LATCH across the wire — a safe sensor must not clear it"
    );
    assert!(
        velocity_magnitude(&still).abs() < 1e-9,
        "a latched ESTOP must zero the command over the wire"
    );

    let _ = client.close().await;
    let _ = server.close().await;
}
