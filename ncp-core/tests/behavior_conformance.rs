//! Behavioral conformance — the ncp-core reference vs the language-neutral
//! decision corpus (`conformance/behavior/vectors.json`).
//!
//! `tests/conformance.rs` pins the *wire shape* (serde <-> schema field parity).
//! This pins *behavior*: for every vector in the corpus, drive the real
//! `ncp_core` function and assert it produces the outcome the corpus declares.
//! Two things follow:
//!   1. the corpus can never claim an outcome the reference does not produce
//!      (this test fails if it drifts), and
//!   2. any other peer that replays the same corpus (e.g. the Python binding via
//!      `scripts/check_behavior_vectors.py`) is checking against outcomes that
//!      are themselves verified against the reference here.
//!
//! Covered functions: `check_version`, `contract_status`, `validate`, and the
//! `SafetyGovernor` (HOLD / ESTOP / speed-clamp / watchdog) decisions.

use ncp_core::{
    check_version, contract_status, validate, CommandFrame, ContractStatus, SafetyGovernor,
    SafetyLimits, SensorFrame, CONTRACT_HASH, NCP_VERSION,
};
use serde_json::Value;
use std::path::PathBuf;

/// Load the corpus from the sibling `conformance/behavior/` directory. The path
/// resolves both pre- and post-extraction (ncp-core stays a workspace member, so
/// the repo-root `conformance/` travels with it via `../..`).
fn load_corpus() -> Value {
    let path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../conformance/behavior"
    ))
    .join("vectors.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read behavior corpus {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("behavior corpus {} is not valid JSON: {e}", path.display()))
}

fn cases<'a>(corpus: &'a Value, function: &str) -> &'a Vec<Value> {
    corpus["cases"][function]
        .as_array()
        .unwrap_or_else(|| panic!("corpus has no array of `{function}` cases"))
}

/// L2 magnitude of the governed command's `velocity_setpoint` channel (0 when the
/// channel is absent — but the HOLD/ESTOP path always re-inserts it zeroed).
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

#[test]
fn check_version_corpus() {
    let corpus = load_corpus();
    for case in cases(&corpus, "check_version") {
        let name = case["name"].as_str().unwrap();
        let version = case["input"]["version"].as_str().unwrap();
        let strict = case["input"]["strict"].as_bool().unwrap();
        let got = check_version(version, strict);
        let expect = &case["expect"];
        if expect["error"].as_bool() == Some(true) {
            assert!(
                got.is_err(),
                "check_version[{name}]: expected an error for {version:?} (strict={strict}), got {got:?}"
            );
        } else {
            let want = expect["compatible"].as_bool().unwrap();
            assert_eq!(
                got.expect("unexpected error"),
                want,
                "check_version[{name}]: {version:?} (strict={strict})"
            );
        }
    }
}

#[test]
fn contract_status_corpus() {
    let corpus = load_corpus();
    for case in cases(&corpus, "contract_status") {
        let name = case["name"].as_str().unwrap();
        let peer = case["input"]["peer_hash"].as_str(); // None when JSON null
        let tag = match contract_status(peer) {
            ContractStatus::Match => "match",
            ContractStatus::NotAdvertised => "not_advertised",
            ContractStatus::Mismatch { .. } => "mismatch",
        };
        assert_eq!(
            tag,
            case["expect"]["status"].as_str().unwrap(),
            "contract_status[{name}]: peer={peer:?}"
        );
    }
}

#[test]
fn validate_corpus() {
    let corpus = load_corpus();
    for case in cases(&corpus, "validate") {
        let name = case["name"].as_str().unwrap();
        let message = &case["input"]["message"];
        let want_valid = case["expect"]["valid"].as_bool().unwrap();
        let got = validate(message);
        assert_eq!(
            got.is_ok(),
            want_valid,
            "validate[{name}]: expected valid={want_valid}, got {got:?}"
        );
    }
}

#[test]
fn govern_corpus() {
    let corpus = load_corpus();
    for case in cases(&corpus, "govern") {
        let name = case["name"].as_str().unwrap();
        let input = &case["input"];
        let limits: SafetyLimits = serde_json::from_value(input["limits"].clone())
            .unwrap_or_else(|e| panic!("govern[{name}]: bad limits: {e}"));
        let command: CommandFrame = serde_json::from_value(input["command"].clone())
            .unwrap_or_else(|e| panic!("govern[{name}]: bad command: {e}"));
        let sensor: Option<SensorFrame> = match input.get("sensor") {
            Some(Value::Null) | None => None,
            Some(s) => Some(
                serde_json::from_value(s.clone())
                    .unwrap_or_else(|e| panic!("govern[{name}]: bad sensor: {e}")),
            ),
        };
        let now_s = input["now_s"].as_f64().unwrap();
        let last_sensor_s = input["last_sensor_s"].as_f64(); // None when null

        // One-shot governor per vector: govern() latches ESTOP on &mut self, but
        // each corpus vector is a single, self-contained decision (latching across
        // calls is covered by safety.rs unit tests, not the cross-language corpus).
        let mut gov = SafetyGovernor::new(limits);
        let out = gov.govern(&command, sensor.as_ref(), now_s, last_sensor_s);
        let out = serde_json::to_value(&out).unwrap();

        assert_eq!(
            out["mode"].as_str().unwrap(),
            case["expect"]["mode"].as_str().unwrap(),
            "govern[{name}]: mode"
        );
        if let Some(want_mag) = case["expect"]["velocity_setpoint_magnitude"].as_f64() {
            let got_mag = velocity_magnitude(&out);
            assert!(
                (got_mag - want_mag).abs() < 1e-9,
                "govern[{name}]: velocity magnitude want {want_mag}, got {got_mag}"
            );
        }
    }
}

#[test]
fn wire_pins_match_corpus_single_source() {
    // SHOULD-FIX #4a: `NCP_VERSION` and `CONTRACT_HASH` are independent hardcoded
    // constants in each peer (Rust here, TS, Python). Pin BOTH to the corpus header
    // so the corpus is the single cross-language source of wire truth — a peer that
    // bumps the wire but forgets the corpus (or vice versa) fails here, the same way
    // `contract_hash_matches_proto` ties the hash to the proto. (The Python binding
    // and ncp-ts assert the same against this corpus; check-version-coherence.sh
    // adds the {ncp-core, ncp-ts, corpus} cross-check.)
    let corpus = load_corpus();
    assert_eq!(
        NCP_VERSION,
        corpus["ncp_version"].as_str().unwrap(),
        "ncp-core NCP_VERSION must equal the behavior corpus ncp_version"
    );
    assert_eq!(
        CONTRACT_HASH,
        corpus["contract_hash"].as_str().unwrap(),
        "ncp-core CONTRACT_HASH must equal the behavior corpus contract_hash"
    );
}
