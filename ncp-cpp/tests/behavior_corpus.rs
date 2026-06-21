//! Behavioral conformance through the C ABI — the C++ SDK vs the language-neutral
//! decision corpus (`conformance/behavior/vectors.json`).
//!
//! `corpus.rs` proves the C ABI agrees on the *wire shape* (every golden message
//! validates). This proves it agrees on *behavior*: the same `check_version` /
//! `contract_status` / `validate` / `govern` decisions the Rust reference is pinned
//! to (`ncp-core/tests/behavior_conformance.rs`) and the Python binding replays
//! (`scripts/check_behavior_vectors.py`). C++ has the full surface, so it runs the
//! whole corpus — a divergence in any C-ABI decision path fails CI here.

use ncp_cpp::{ncp_check_version, ncp_contract_status, ncp_govern, ncp_string_free, ncp_validate};
use serde_json::Value;
use std::ffi::{CStr, CString};
use std::path::PathBuf;

fn load_corpus() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../conformance/behavior/vectors.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read behavior corpus {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("behavior corpus is not JSON: {e}"))
}

fn cases<'a>(corpus: &'a Value, function: &str) -> &'a Vec<Value> {
    corpus["cases"][function]
        .as_array()
        .unwrap_or_else(|| panic!("corpus has no `{function}` cases"))
}

fn velocity_magnitude(frame: &Value) -> f64 {
    frame["channels"]["velocity_setpoint"]["data"]
        .as_array()
        .map(|d| {
            d.iter()
                .filter_map(Value::as_f64)
                .map(|c| c * c)
                .sum::<f64>()
                .sqrt()
        })
        .unwrap_or(0.0)
}

#[test]
fn check_version_through_the_c_abi() {
    let corpus = load_corpus();
    for case in cases(&corpus, "check_version") {
        let name = case["name"].as_str().unwrap();
        let version = CString::new(case["input"]["version"].as_str().unwrap()).unwrap();
        let strict = case["input"]["strict"].as_bool().unwrap();
        // C ABI: 1 = compatible, 0 = incompatible, -1 = unparseable OR strict-mismatch.
        let got = unsafe { ncp_check_version(version.as_ptr(), strict) };
        let expect = &case["expect"];
        if expect["error"].as_bool() == Some(true) {
            assert_eq!(got, -1, "check_version[{name}]: expected -1 (error)");
        } else {
            let want = if expect["compatible"].as_bool().unwrap() {
                1
            } else {
                0
            };
            assert_eq!(got, want, "check_version[{name}]");
        }
    }
}

#[test]
fn contract_status_through_the_c_abi() {
    let corpus = load_corpus();
    for case in cases(&corpus, "contract_status") {
        let name = case["name"].as_str().unwrap();
        // C ABI: 1 = match, 0 = not advertised (NULL), 2 = mismatch.
        let want = match case["expect"]["status"].as_str().unwrap() {
            "match" => 1,
            "not_advertised" => 0,
            "mismatch" => 2,
            other => panic!("unknown status {other:?}"),
        };
        let got = match case["input"]["peer_hash"].as_str() {
            None => unsafe { ncp_contract_status(std::ptr::null()) },
            Some(h) => {
                let c = CString::new(h).unwrap();
                unsafe { ncp_contract_status(c.as_ptr()) }
            }
        };
        assert_eq!(got, want, "contract_status[{name}]");
    }
}

#[test]
fn validate_through_the_c_abi() {
    let corpus = load_corpus();
    for case in cases(&corpus, "validate") {
        let name = case["name"].as_str().unwrap();
        let kind = CString::new(case["input"]["kind"].as_str().unwrap()).unwrap();
        let json = CString::new(serde_json::to_string(&case["input"]["message"]).unwrap()).unwrap();
        // C ABI: non-NULL canonical JSON = accept, NULL = reject.
        let out = unsafe { ncp_validate(kind.as_ptr(), json.as_ptr()) };
        let valid = !out.is_null();
        if !out.is_null() {
            unsafe { ncp_string_free(out) };
        }
        assert_eq!(
            valid,
            case["expect"]["valid"].as_bool().unwrap(),
            "validate[{name}]"
        );
    }
}

#[test]
fn govern_through_the_c_abi() {
    let corpus = load_corpus();
    for case in cases(&corpus, "govern") {
        let name = case["name"].as_str().unwrap();
        let input = &case["input"];
        let limits = CString::new(serde_json::to_string(&input["limits"]).unwrap()).unwrap();
        let command = CString::new(serde_json::to_string(&input["command"]).unwrap()).unwrap();
        let now_s = input["now_s"].as_f64().unwrap();
        // C ABI convention: last_sensor_s < 0.0 means "no last sensor" (None).
        let last_sensor_s = input["last_sensor_s"].as_f64().unwrap_or(-1.0);
        let sensor = match input.get("sensor") {
            Some(Value::Null) | None => None,
            Some(s) => Some(CString::new(serde_json::to_string(s).unwrap()).unwrap()),
        };
        let sensor_ptr = sensor.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());

        let out = unsafe {
            ncp_govern(
                limits.as_ptr(),
                command.as_ptr(),
                now_s,
                sensor_ptr,
                last_sensor_s,
            )
        };
        assert!(!out.is_null(), "govern[{name}]: returned NULL");
        let governed: Value = {
            let s = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
            unsafe { ncp_string_free(out) };
            serde_json::from_str(&s).unwrap()
        };
        assert_eq!(
            governed["mode"].as_str().unwrap(),
            case["expect"]["mode"].as_str().unwrap(),
            "govern[{name}]: mode"
        );
        if let Some(want_mag) = case["expect"]["velocity_setpoint_magnitude"].as_f64() {
            let got_mag = velocity_magnitude(&governed);
            assert!(
                (got_mag - want_mag).abs() < 1e-9,
                "govern[{name}]: |velocity| want {want_mag}, got {got_mag}"
            );
        }
    }
}
