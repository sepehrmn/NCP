//! Cross-language conformance: every golden JSON vector in the shared corpus
//! (`conformance/vectors/*.json`) must validate through the C ABI `ncp_validate`.
//! This proves the C++ SDK agrees with the SAME language-agnostic vectors the
//! Python `validate` and the Rust serde/schema guards check — so a divergence in
//! any one binding's wire handling fails CI, not a downstream integration.

use ncp_cpp::{ncp_string_free, ncp_validate};
use std::ffi::{CStr, CString};
use std::path::PathBuf;

/// Drive one (kind, json) pair through the C ABI; `Some(canonical_json)` on
/// accept, `None` on the NULL sentinel (reject), freeing via the FFI path.
unsafe fn validate(kind: &str, json: &str) -> Option<String> {
    let k = CString::new(kind).unwrap();
    let j = CString::new(json).unwrap();
    let out = ncp_validate(k.as_ptr(), j.as_ptr());
    if out.is_null() {
        return None;
    }
    let s = CStr::from_ptr(out).to_str().unwrap().to_string();
    ncp_string_free(out);
    Some(s)
}

fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../conformance/vectors")
}

#[test]
fn every_json_vector_validates_through_the_c_abi() {
    let dir = vectors_dir();
    let mut n = 0;
    for entry in std::fs::read_dir(&dir).expect("conformance/vectors readable") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue; // skip the binary bulk vectors (*.bin)
        }
        let json = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let kind = v["kind"].as_str().expect("vector carries a string `kind`");
        let out = unsafe { validate(kind, &json) };
        assert!(
            out.is_some(),
            "vector {:?} (kind {kind}) must validate through ncp_validate",
            path.file_name().unwrap()
        );
        n += 1;
    }
    assert!(
        n >= 13,
        "expected the full JSON corpus (>=13 vectors), saw {n} — did coverage drop?"
    );
}

#[test]
fn tampered_scientific_boundary_is_rejected_through_the_c_abi() {
    // A frame asserting it is a calibrated posterior must be rejected by the C++
    // SDK exactly as by the Rust reference (the ncp_core::validate value-pin).
    let lie = r#"{"kind":"observation_frame","session_id":"s1","calibrated_posterior":true}"#;
    assert!(
        unsafe { validate("observation_frame", lie) }.is_none(),
        "calibrated_posterior=true must be rejected through the C ABI"
    );
    // The honest frame (discriminators absent → safe defaults) is accepted.
    let honest = r#"{"kind":"observation_frame","session_id":"s1"}"#;
    assert!(unsafe { validate("observation_frame", honest) }.is_some());
}

#[test]
fn missing_required_is_rejected_through_the_c_abi() {
    // A step_request without its required session_id must reject (the canonical
    // ncp_core::validate check the typed round-trip alone would silently default).
    let bad = r#"{"kind":"step_request","advance_ms":1.0}"#;
    assert!(unsafe { validate("step_request", bad) }.is_none());
    let good = r#"{"kind":"step_request","session_id":"s1"}"#;
    assert!(unsafe { validate("step_request", good) }.is_some());
}
