//! A minimal **Rust NCP client** that drives an NCP server over a plain TCP socket
//! with newline-delimited JSON — the medium the `ncp-gateway` bridges to a Python
//! backend. Run against the reference Python server (engram's `bridge_server
//! --backend mock`) to prove a Rust consumer (e.g. a crebain/pid_vla peer) interops
//! with the Python server over a real cross-process, cross-language boundary:
//!
//! ```text
//! python -m backend.neurocontrol.bridge_server --backend mock --port 28480 &  # in Paper2Brain
//! cargo run -p ncp-core --example ncp_tcp_client -- 28480                      # in NCP
//! ```
//!
//! It builds requests from the canonical `ncp_core` message types, runs the real
//! version + advisory-contract handshake on the reply, and asserts the
//! scientific-boundary discriminators — exiting non-zero on any contract violation,
//! so an orchestrator can gate on it.

use ncp_core::{
    check_version, contract_status, message_kind, validate, CloseSession, ContractStatus,
    NetworkRef, NetworkRefKind, OpenSession, StepRequest,
};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("RUST TCP CLIENT FAIL: {msg}");
    std::process::exit(1);
}

/// Send one NCP message (serialized + newline) and read one reply line, validating it.
fn rpc(
    stream: &mut TcpStream,
    reader: &mut impl BufRead,
    msg: &impl serde::Serialize,
) -> serde_json::Value {
    let line = serde_json::to_string(msg).unwrap_or_else(|e| fail(format!("serialize: {e}")));
    stream
        .write_all(format!("{line}\n").as_bytes())
        .unwrap_or_else(|e| fail(format!("write: {e}")));
    stream.flush().ok();
    let mut buf = String::new();
    if reader.read_line(&mut buf).unwrap_or(0) == 0 {
        fail("server closed the connection without a reply");
    }
    let reply: serde_json::Value =
        serde_json::from_str(buf.trim()).unwrap_or_else(|e| fail(format!("parse reply: {e}")));
    if message_kind(&reply) == Some("error") {
        fail(format!("server returned error frame: {reply}"));
    }
    // The Rust peer validates inbound frames against the same wire contract.
    validate(&reply).unwrap_or_else(|e| fail(format!("inbound frame fails validate(): {e}")));
    reply
}

fn assert_boundary(carrier: &serde_json::Value, ctx: &str) {
    if carrier
        .get("is_simulation_output")
        .and_then(|v| v.as_bool())
        != Some(true)
    {
        fail(format!("{ctx}: is_simulation_output must be true"));
    }
    if carrier
        .get("calibrated_posterior")
        .and_then(|v| v.as_bool())
        != Some(false)
    {
        fail(format!("{ctx}: calibrated_posterior must be false"));
    }
}

fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "28474".into())
        .parse()
        .unwrap_or_else(|e| fail(format!("bad port: {e}")));
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).unwrap_or_else(|e| fail(format!("connect: {e}")));
    let mut reader = BufReader::new(stream.try_clone().unwrap());

    // open
    let opened = rpc(
        &mut stream,
        &mut reader,
        &OpenSession {
            session_id: "rust-cli".into(),
            network: NetworkRef {
                kind: NetworkRefKind::Builtin,
                ref_: "iaf_psc_alpha".into(),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    if message_kind(&opened) != Some("session_opened") || opened["ok"].as_bool() != Some(true) {
        fail(format!("expected session_opened ok, got {opened}"));
    }
    // The Rust peer's handshake: hard version gate + advisory contract check.
    let ver = opened["ncp_version"].as_str().unwrap_or("");
    if !check_version(ver, false).unwrap_or(false) {
        fail(format!("server ncp_version {ver:?} incompatible"));
    }
    if let ContractStatus::Mismatch { peer } = contract_status(opened["contract_hash"].as_str()) {
        eprintln!("[rust-cli] advisory: peer contract_hash {peer:?} differs (proceeding)");
    }
    assert_boundary(&opened["provenance"], "session_opened.provenance");

    // step
    let obs = rpc(
        &mut stream,
        &mut reader,
        &StepRequest {
            session_id: "rust-cli".into(),
            advance_ms: Some(10.0),
            ..Default::default()
        },
    );
    if message_kind(&obs) != Some("observation_frame") {
        fail(format!("expected observation_frame, got {obs}"));
    }
    assert_boundary(&obs, "observation_frame");

    // close
    let closed = rpc(
        &mut stream,
        &mut reader,
        &CloseSession {
            session_id: "rust-cli".into(),
            ..Default::default()
        },
    );
    if message_kind(&closed) != Some("session_closed") || closed["ok"].as_bool() != Some(true) {
        fail(format!("expected session_closed ok, got {closed}"));
    }

    println!("RUST TCP CLIENT OK: open→step→close round-trip vs the Python NCP server, contract verified");
}
