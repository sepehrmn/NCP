//! Wire conformance test — guards against Rust <-> JSON-schema drift.
//!
//! For every NCP message type that has a JSON schema in `ncp/schemas/`, this
//! test:
//!   1. constructs a representative Rust instance,
//!   2. serializes it with `serde_json` to a `serde_json::Value` object,
//!   3. loads the corresponding schema from `ncp/schemas/<name>.schema.json`,
//!   4. asserts field-set parity: every key in the serialized JSON exists in
//!      the schema `properties` (catches renamed / extra fields, i.e. drift),
//!      and every name in the schema `required` array is present in the
//!      serialized JSON.
//!
//! This is intentionally *not* full JSON-Schema validation (no external crate):
//! field-set + required parity is enough to catch wire drift and is
//! dependency-free (only `serde` / `serde_json`, both normal ncp-core deps).
//!
//! The schema dir is located via `CARGO_MANIFEST_DIR/../schemas`, which resolves
//! to `ncp/schemas` both pre- and post-extraction (ncp-core stays a workspace
//! member, so the sibling `schemas/` directory travels with it).

use ncp_core::messages::*;
use serde_json::Value;
use std::path::PathBuf;

/// Load `<name>.schema.json` from the sibling `ncp/schemas/` directory.
fn load_schema(name: &str) -> Value {
    let path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../schemas"))
        .join(format!("{name}.schema.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read schema {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("schema {} is not valid JSON: {e}", path.display()))
}

/// Assert field-set parity between a serialized message and its schema.
///
/// `name` is the schema basename (e.g. `"open_session"`); `value` is the
/// serialized message (must be a JSON object).
fn assert_parity<T: serde::Serialize>(name: &str, msg: &T) {
    let value =
        serde_json::to_value(msg).unwrap_or_else(|e| panic!("{name}: serialization failed: {e}"));
    let obj = value
        .as_object()
        .unwrap_or_else(|| panic!("{name}: serialized message is not a JSON object"));

    let schema = load_schema(name);
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("{name}: schema has no top-level `properties` object"));

    // (a) Every serialized key must be a known schema property. This is the
    //     drift catcher: a renamed Rust field (or an extra one) shows up here.
    for key in obj.keys() {
        assert!(
            properties.contains_key(key),
            "{name}: serialized field {key:?} is not in schema `properties` \
             (Rust<->schema drift — a renamed or extra field). \
             schema properties: {:?}",
            properties.keys().collect::<Vec<_>>()
        );
    }

    // (b) Every schema-required name must be present in the serialized JSON.
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for req in required {
            let req = req
                .as_str()
                .unwrap_or_else(|| panic!("{name}: non-string entry in `required`"));
            assert!(
                obj.contains_key(req),
                "{name}: schema-required field {req:?} is missing from the \
                 serialized message. serialized keys: {:?}",
                obj.keys().collect::<Vec<_>>()
            );
        }
    }

    // (c) Reverse direction: every schema `properties` key must appear in the
    //     serialized object. Because the message types are `#[serde(default)]`
    //     (no field is skipped on serialize), a property declared in the schema
    //     but dropped from the Rust struct shows up here — the dual of (a).
    for prop in properties.keys() {
        assert!(
            obj.contains_key(prop),
            "{name}: schema property {prop:?} is absent from the serialized \
             message (Rust<->schema drift — a dropped field). serialized keys: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
    }
}

#[test]
fn open_session_conforms() {
    assert_parity("open_session", &OpenSession::default());
}

#[test]
fn session_opened_conforms() {
    assert_parity("session_opened", &SessionOpened::default());
}

#[test]
fn stimulus_frame_conforms() {
    assert_parity("stimulus_frame", &StimulusFrame::default());
}

#[test]
fn step_request_conforms() {
    assert_parity("step_request", &StepRequest::default());
}

#[test]
fn run_request_conforms() {
    assert_parity("run_request", &RunRequest::default());
}

#[test]
fn observation_frame_conforms() {
    assert_parity("observation_frame", &ObservationFrame::default());
}

#[test]
fn close_session_conforms() {
    assert_parity("close_session", &CloseSession::default());
}

#[test]
fn session_closed_conforms() {
    assert_parity("session_closed", &SessionClosed::default());
}

#[test]
fn capabilities_conforms() {
    assert_parity("capabilities", &Capabilities::default());
}

#[test]
fn sensor_frame_conforms() {
    assert_parity("sensor_frame", &SensorFrame::default());
}

#[test]
fn command_frame_conforms() {
    assert_parity("command_frame", &CommandFrame::default());
}

#[test]
fn control_status_conforms() {
    assert_parity("control_status", &ControlStatus::default());
}

#[test]
fn link_status_conforms() {
    assert_parity("link_status", &LinkStatus::default());
}

/// Targeted: `NetworkRef.ref_` must serialize on the wire as `"ref"`, never
/// `"ref_"`. `ref` is a Rust keyword, so the struct field is renamed; if that
/// `#[serde(rename = "ref")]` is ever dropped this guards the regression.
#[test]
fn network_ref_serializes_ref_not_ref_underscore() {
    let value = serde_json::to_value(NetworkRef::default()).unwrap();
    let obj = value.as_object().expect("NetworkRef is a JSON object");
    assert!(
        obj.contains_key("ref"),
        "NetworkRef must serialize the field as \"ref\"; keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
    assert!(
        !obj.contains_key("ref_"),
        "NetworkRef must NOT serialize \"ref_\" (the Rust field name leaked); keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
}

/// Targeted: `ObservationFrame` must carry the scientific-boundary
/// discriminators (`calibrated_posterior=false`, `is_simulation_output=true`)
/// and a `seq` field — returned data is raw simulation output, never a
/// calibrated reproduction, and observers align on `seq`.
#[test]
fn observation_frame_carries_provenance_and_seq() {
    let value = serde_json::to_value(ObservationFrame::default()).unwrap();
    let obj = value
        .as_object()
        .expect("ObservationFrame is a JSON object");

    assert_eq!(
        obj.get("calibrated_posterior"),
        Some(&Value::Bool(false)),
        "ObservationFrame.calibrated_posterior must default to false"
    );
    assert_eq!(
        obj.get("is_simulation_output"),
        Some(&Value::Bool(true)),
        "ObservationFrame.is_simulation_output must default to true"
    );
    assert!(
        obj.contains_key("seq"),
        "ObservationFrame must carry a `seq` field; keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
}

/// Nested-struct coverage: a `Default::default()` instance has empty `Vec`s and
/// `BTreeMap`s, so the nested message structs (`NetworkRef`, `RecordTarget`, …)
/// never actually serialize and their fields go unchecked. Construct a
/// *non-default* `OpenSession` carrying a populated `NetworkRef`, `RecordSpec`
/// and `StimulusSpec`, and assert the nested objects serialize the full key set
/// the schema `$defs` declare. Top-level parity is still covered by
/// `open_session_conforms`; this guards the nested types against drift.
#[test]
fn open_session_nested_structs_serialize_full_keys() {
    let open = OpenSession {
        session_id: "uav3-percept".into(),
        network: NetworkRef {
            kind: NetworkRefKind::Handle,
            ref_: "compiled-mod-1".into(),
            model_name: Some("iaf_psc_alpha".into()),
            population_sizes: [("feat".to_string(), 8)].into_iter().collect(),
            params: [("tau_m".to_string(), 10.0)].into_iter().collect(),
        },
        record: RecordSpec {
            targets: vec![RecordTarget {
                port: "spk".into(),
                target: "feat".into(),
                observable: Observable::Spikes,
                ids: vec![1, 2, 3],
                cadence_ms: 2.0,
                recordables: Vec::new(),
            }],
        },
        stimulus: StimulusSpec {
            targets: vec![StimulusTarget {
                port: "drive".into(),
                target: "feat".into(),
                kind: StimulusKind::CurrentPa,
                ids: vec![1],
                params: Default::default(),
            }],
        },
        ..Default::default()
    };

    let value = serde_json::to_value(&open).unwrap();

    // Nested NetworkRef: must serialize `ref` (not `ref_`) and all its fields.
    let net = value["network"].as_object().expect("network is an object");
    for key in ["kind", "ref", "model_name", "population_sizes", "params"] {
        assert!(
            net.contains_key(key),
            "NetworkRef must serialize {key:?}; keys: {:?}",
            net.keys().collect::<Vec<_>>()
        );
    }
    assert!(
        !net.contains_key("ref_"),
        "NetworkRef must not leak the Rust field name `ref_`"
    );

    // Nested RecordTarget.
    let rt = value["record"]["targets"][0]
        .as_object()
        .expect("record target is an object");
    for key in ["port", "target", "observable", "ids", "cadence_ms"] {
        assert!(
            rt.contains_key(key),
            "RecordTarget must serialize {key:?}; keys: {:?}",
            rt.keys().collect::<Vec<_>>()
        );
    }
    assert_eq!(rt["observable"], Value::String("spikes".into()));

    // Nested StimulusTarget.
    let st = value["stimulus"]["targets"][0]
        .as_object()
        .expect("stimulus target is an object");
    for key in ["port", "target", "kind", "ids"] {
        assert!(
            st.contains_key(key),
            "StimulusTarget must serialize {key:?}; keys: {:?}",
            st.keys().collect::<Vec<_>>()
        );
    }
    assert_eq!(st["kind"], Value::String("current_pA".into()));
}
