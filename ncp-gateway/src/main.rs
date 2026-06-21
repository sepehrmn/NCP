#![doc = include_str!("../README.md")]
//!
//! # Configuration and security
//!
//! Config via env:
//!   NCP_REALM        key-expression realm           (default `ncp`; set per deployment)
//!   NCP_BRIDGE_ADDR  Python bridge_server.py addr    (default `127.0.0.1:28474`)
//!   NCP_ZENOH_CONFIG path to a Zenoh ACL/TLS config  (default: hardened, scouting off)
//!
//! Security: the realm is *addressing*, not a credential. By default this gateway
//! opens the hardened config (multicast scouting disabled). For an enforced
//! deployment set `NCP_ZENOH_CONFIG` to the shipped per-plane ACL config
//! (`deploy/zenoh-access-control.json5`) paired with mutual TLS; if it is set but the
//! file is missing/malformed the gateway refuses to start (fail-closed).

use ncp_core::keys::Keys;
use ncp_zenoh::{ZenohBus, NCP_ZENOH_CONFIG_ENV};
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

    // Honor NCP_ZENOH_CONFIG explicitly: when set, load the shipped ACL/TLS config
    // file (fail-closed — a missing/malformed file aborts startup). When unset, fall
    // back to the hardened default (multicast scouting off). The realm is addressing,
    // not a credential — enforcement comes from this config, not the realm string.
    let keys = Keys::new(realm.clone());
    let open = match std::env::var_os(NCP_ZENOH_CONFIG_ENV) {
        Some(path) => {
            println!("[ncp-gateway] loading Zenoh config from {NCP_ZENOH_CONFIG_ENV}={path:?}");
            ZenohBus::with_config_file(std::path::Path::new(&path), keys).await
        }
        None => {
            eprintln!(
                "[ncp-gateway] {NCP_ZENOH_CONFIG_ENV} unset: opening hardened default \
                 (multicast scouting OFF, no ACL/TLS). The realm is addressing, not a \
                 credential — set {NCP_ZENOH_CONFIG_ENV} to deploy/zenoh-access-control.json5 \
                 (with mTLS) for an enforced deployment. See SECURITY.md."
            );
            ZenohBus::open_realm(keys).await
        }
    };
    let bus = match open {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[ncp-gateway] failed to open Zenoh session: {e}");
            std::process::exit(1);
        }
    };

    let addr = bridge_addr.clone();
    let serve = bus
        .serve_rpc(move |req: Vec<u8>| {
            // forward_to_python is blocking std::net I/O (30s timeouts). block_in_place
            // frees this tokio worker so other tasks on the shared multi-thread runtime
            // (data planes, Zenoh internals) aren't starved while the bridge round-trip
            // is in flight. Valid here: ncp-gateway runs on the multi-thread runtime.
            tokio::task::block_in_place(|| match forward_to_python(&addr, &req) {
                Ok(reply) if !reply.is_empty() => reply,
                Ok(_) => error_frame("empty reply from Python bridge"),
                Err(e) => error_frame(&format!("bridge unreachable at {addr}: {e}")),
            })
        })
        .await;
    if let Err(e) = serve {
        eprintln!("[ncp-gateway] failed to declare RPC queryable: {e}");
        std::process::exit(1);
    }

    println!(
        "[ncp-gateway] serving NCP RPC on Zenoh key '{realm}/rpc' → Python bridge {bridge_addr}"
    );
    println!("[ncp-gateway] observation/sensor/command planes: '{realm}/session/<id>/<plane>'");
    println!("[ncp-gateway] Ctrl-C to stop.");
    let _ = tokio::signal::ctrl_c().await;
    let _ = bus.close().await;
    println!("[ncp-gateway] stopped.");
}
