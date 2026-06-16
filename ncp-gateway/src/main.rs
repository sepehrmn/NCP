//! # ncp-gateway — Engram's Rust NCP edge
//!
//! Engram's brain is NEST (Python), so its NCP *server* stays Python. This
//! gateway gives Engram a production-grade **Rust Zenoh edge**: it runs the
//! control-plane RPC queryable (`{realm}/rpc`) and the observation pub/sub, and
//! bridges each RPC to the Python `SessionService` over a localhost socket —
//! reusing the existing transport-neutral `handle_json` seam. The fleet-facing,
//! latency-sensitive transport becomes Rust (SHM/QoS, many-to-many discovery,
//! free observer taps); `nest.Run` stays in Python.
//!
//! ```text
//!  Zenoh bus  ──(SHM/QoS)──►  ncp-gateway (this)  ──(TCP, newline-JSON)──►  bridge_server.py
//!     ▲                          {realm}/rpc queryable                       SessionService.handle_json → nest.Run
//!     └── crebain, pid_vla, dashboards attach as peers / observers
//! ```
//!
//! Config via env:
//!   NCP_REALM        key-expression realm           (default `engram/ncp`)
//!   NCP_BRIDGE_ADDR  Python bridge_server.py addr    (default `127.0.0.1:28474`)

use ncp_core::keys::Keys;
use ncp_zenoh::ZenohBus;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

const DEFAULT_BRIDGE_ADDR: &str = "127.0.0.1:28474";

/// Forward one NCP request (JSON bytes) to the Python bridge and return the reply
/// (JSON bytes). Newline-delimited JSON, one request → one reply. Blocking — the
/// control-plane RPC is rare (session lifecycle), never the per-tick hot path.
fn forward_to_python(addr: &str, request: &[u8]) -> std::io::Result<Vec<u8>> {
    let stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut writer = stream.try_clone()?;
    writer.write_all(request)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line)?;
    while line.last() == Some(&b'\n') || line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(line)
}

fn error_frame(message: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "kind": "error", "error": message }))
        .unwrap_or_else(|_| br#"{"kind":"error","error":"serialization failed"}"#.to_vec())
}

#[tokio::main]
async fn main() {
    let realm = std::env::var("NCP_REALM").unwrap_or_else(|_| ncp_core::DEFAULT_REALM.to_string());
    let bridge_addr =
        std::env::var("NCP_BRIDGE_ADDR").unwrap_or_else(|_| DEFAULT_BRIDGE_ADDR.to_string());

    let bus = match ZenohBus::open_realm(Keys::new(realm.clone())).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[ncp-gateway] failed to open Zenoh session: {e}");
            std::process::exit(1);
        }
    };

    let addr = bridge_addr.clone();
    let serve = bus
        .serve_rpc(move |req: Vec<u8>| match forward_to_python(&addr, &req) {
            Ok(reply) if !reply.is_empty() => reply,
            Ok(_) => error_frame("empty reply from Python bridge"),
            Err(e) => error_frame(&format!("bridge unreachable at {addr}: {e}")),
        })
        .await;
    if let Err(e) = serve {
        eprintln!("[ncp-gateway] failed to declare RPC queryable: {e}");
        std::process::exit(1);
    }

    println!("[ncp-gateway] serving NCP RPC on Zenoh key '{realm}/rpc' → Python bridge {bridge_addr}");
    println!("[ncp-gateway] observation/sensor/command planes: '{realm}/session/<id>/<plane>'");
    println!("[ncp-gateway] Ctrl-C to stop.");
    let _ = tokio::signal::ctrl_c().await;
    bus.close();
    println!("[ncp-gateway] stopped.");
}
