//! Cross-session NCP RPC over a REAL Zenoh tcp link.
//!
//! `loopback.rs` proves `ncp-zenoh` runs by delivering a session's own publications
//! to its own subscribers (one in-process session). This goes further: two
//! INDEPENDENT `ZenohBus` sessions — one **listens** on a localhost tcp endpoint, the
//! other **connects** to it — so every request/reply crosses Zenoh's actual transport
//! between sessions (the path a two-process deployment uses, with multicast discovery
//! off). It drives the full control-plane lifecycle (open → step → run → close)
//! through the typed `ZenohNcpClient` (which performs the version + advisory-contract
//! handshake on the reply) against a NEST-free `serve_rpc` handler, asserting the wire
//! contract — handshake, the scientific-boundary discriminators, the lifecycle, the
//! recorded port — survives the production medium end to end.

use ncp_core::keys::Keys;
use ncp_core::{
    CloseSession, NetworkRef, NetworkRefKind, OpenSession, RunRequest, StepRequest, CONTRACT_HASH,
    NCP_VERSION,
};
use ncp_zenoh::{ZenohBus, ZenohConfig, ZenohNcpClient};
use serde_json::{json, Value};
use std::time::Duration;

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

/// A NEST-free mock NCP server: maps each control-plane request to its reply, with
/// the mandatory scientific-boundary discriminators set. A real backend would advance
/// a kernel here; the wire contract is identical either way (that is the point — the
/// transport carries the contract regardless of the simulator behind it).
fn mock_handler(req: Vec<u8>) -> Vec<u8> {
    let v: Value = serde_json::from_slice(&req).unwrap_or_else(|_| json!({}));
    let sid = v.get("session_id").and_then(Value::as_str).unwrap_or("s");
    let kind = v.get("kind").and_then(Value::as_str).unwrap_or("");
    let reply = match kind {
        "open_session" => json!({
            "kind": "session_opened", "ncp_version": NCP_VERSION, "session_id": sid, "ok": true,
            "backend": "mock", "contract_hash": CONTRACT_HASH,
            "provenance": {"network_ref": "x", "backend": "mock", "calibrated_posterior": false,
                           "is_simulation_output": true, "advisory_only": true},
        }),
        "step_request" | "run_request" => json!({
            "kind": "observation_frame", "ncp_version": NCP_VERSION, "session_id": sid,
            "calibrated_posterior": false, "is_simulation_output": true,
            "records": {"vm": {"port": "vm", "target": "pop", "observable": "V_m",
                               "times": [1.0], "values": [-65.0], "unit": "mV"}},
        }),
        "close_session" => {
            json!({"kind": "session_closed", "ncp_version": NCP_VERSION, "session_id": sid, "ok": true})
        }
        other => {
            json!({"kind": "error", "session_id": sid, "error": format!("unknown kind {other}")})
        }
    };
    serde_json::to_vec(&reply).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ncp_rpc_over_real_zenoh_tcp_link() {
    let port = free_port();

    // Server session LISTENS on a localhost tcp endpoint and serves the RPC queryable.
    let server = ZenohBus::with_config(listen_cfg(port), Keys::default())
        .await
        .expect("open server session (listen)");
    server.serve_rpc(mock_handler).await.expect("serve_rpc");

    // Client session CONNECTS to it — a separate session, real tcp transport between them.
    let client_bus = ZenohBus::with_config(connect_cfg(port), Keys::default())
        .await
        .expect("open client session (connect)");
    // Let the link + queryable declaration propagate across the tcp link.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let client = ZenohNcpClient::new(client_bus);

    // open — includes the version gate + advisory contract-hash handshake on the reply.
    let opened = client
        .open(&OpenSession {
            session_id: "uav1".into(),
            network: NetworkRef {
                kind: NetworkRefKind::Builtin,
                ref_: "iaf_psc_alpha".into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .expect("open over zenoh tcp link");
    assert!(opened.ok, "session must open");
    let prov = opened
        .provenance
        .expect("session_opened carries provenance");
    assert!(
        prov.is_simulation_output && !prov.calibrated_posterior,
        "scientific boundary must hold over the wire"
    );

    // step — observation_frame with the boundary discriminators + the recorded port.
    let obs = client
        .step(&StepRequest {
            session_id: "uav1".into(),
            advance_ms: Some(10.0),
            ..Default::default()
        })
        .await
        .expect("step over zenoh");
    assert!(
        obs.is_simulation_output && !obs.calibrated_posterior,
        "observation_frame boundary must hold"
    );
    assert!(
        obs.records.contains_key("vm"),
        "the recorded port must come back across the link"
    );

    // run — batch advance.
    let run = client
        .run(&RunRequest {
            session_id: "uav1".into(),
            duration_ms: 50.0,
            ..Default::default()
        })
        .await
        .expect("run over zenoh");
    assert!(run.is_simulation_output, "run observation_frame boundary");

    // close.
    let closed = client
        .close(&CloseSession {
            session_id: "uav1".into(),
            ..Default::default()
        })
        .await
        .expect("close over zenoh");
    assert!(closed.ok, "session must close");
}
