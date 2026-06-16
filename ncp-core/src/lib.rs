//! # ncp-core — Neuro-Cybernetic Protocol (NCP), Rust reference
//!
//! NCP is a versioned, transport-agnostic, project-agnostic standard for letting
//! an Engram-driven NEST simulation serve external robot / UAV / simulation
//! systems — for **perception, action, both, or neither**. This crate is the
//! normative Rust core: the wire types, the version guard, the key scheme, a
//! reference rate codec, the action-plane safety governor, and an in-process bus
//! and control loop. The Zenoh transport is in `ncp-zenoh`.
//!
//! Scientific boundary (binding): returned `V_m`/spikes are **raw simulation
//! outputs of a specified model**, never a validated reproduction. Every
//! [`ObservationFrame`] carries `calibrated_posterior=false` and
//! `is_simulation_output=true`. A neuro-controller is a control artifact, never a
//! paper-reproduction claim.
//!
//! The wire types serialize to semantically-equivalent JSON to what the Python
//! reference and the protobuf IDL produce (map key order may differ between
//! encoders), so Rust, Python and TypeScript peers interoperate.
//!
//! ```
//! use ncp_core::{OpenSession, NetworkRef, NetworkRefKind, RecordSpec, RecordTarget, Observable};
//! let open = OpenSession {
//!     session_id: "uav3-percept".into(),
//!     network: NetworkRef {
//!         kind: NetworkRefKind::Builtin,
//!         ref_: "iaf_psc_alpha".into(),
//!         population_sizes: [("feat".to_string(), 1)].into_iter().collect(),
//!         ..Default::default()
//!     },
//!     record: RecordSpec { targets: vec![RecordTarget {
//!         port: "spk".into(), target: "feat".into(), observable: Observable::Spikes, ..Default::default()
//!     }] },
//!     ..Default::default()
//! };
//! let json = serde_json::to_string(&open).unwrap();
//! assert!(json.contains("\"kind\":\"open_session\""));
//! ```

pub mod bus;
pub mod codec;
pub mod keys;
pub mod messages;
pub mod resilience;
pub mod safety;
pub mod transport;

pub use bus::{Bus, BusError, LocalBus, NcpBusClient, NcpBusServer, QueryHandler, SubCallback};
pub use codec::{default_uav_velocity_codec, CodecSpec, DecoderChannelMap, EncoderChannelMap};
pub use keys::{valid_id_segment, Keys, DEFAULT_REALM};
pub use messages::*;
pub use resilience::{max_horizon_len, ActionBuffer, LinkMonitor};
pub use safety::{CommandWatchdog, SafetyGovernor};
pub use transport::{
    ControlTransport, Controller, InProcessTransport, NeuroControlLoop, ReflexController,
};

#[cfg(test)]
mod wire_tests {
    use super::*;

    /// The `kind` discriminator and enum string values must match the Python
    /// reference exactly so peers interoperate.
    #[test]
    fn enum_wire_values() {
        assert_eq!(serde_json::to_string(&Observable::Vm).unwrap(), "\"V_m\"");
        assert_eq!(
            serde_json::to_string(&Observable::Spikes).unwrap(),
            "\"spikes\""
        );
        assert_eq!(
            serde_json::to_string(&StimulusKind::CurrentPa).unwrap(),
            "\"current_pA\""
        );
        assert_eq!(
            serde_json::to_string(&StimulusKind::SpikeTimes).unwrap(),
            "\"spike_times\""
        );
        assert_eq!(
            serde_json::to_string(&NetworkRefKind::ModelId).unwrap(),
            "\"model_id\""
        );
        assert_eq!(serde_json::to_string(&Mode::Estop).unwrap(), "\"estop\"");
    }

    /// A step request from a TS/Python client must round-trip through the Rust
    /// types (forward-compatible: unknown fields ignored).
    #[test]
    fn step_request_roundtrip_from_python_json() {
        let json = r#"{
            "ncp_version": "0.1",
            "kind": "step_request",
            "session_id": "s1",
            "advance_ms": 50.0,
            "stimulus": {"kind":"stimulus_frame","session_id":"s1","values":{
                "drive": {"data":[500.0],"unit":"pA"}
            }},
            "future_field_we_do_not_know": 7
        }"#;
        let req: StepRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.session_id, "s1");
        assert_eq!(req.advance_ms, Some(50.0));
        let stim = req.stimulus.unwrap();
        assert_eq!(stim.values["drive"].data, vec![500.0]);
        assert_eq!(stim.values["drive"].unit.as_deref(), Some("pA"));
    }

    #[test]
    fn observation_frame_carries_scientific_boundary() {
        let obs = ObservationFrame::default();
        let v: serde_json::Value = serde_json::to_value(&obs).unwrap();
        assert_eq!(v["calibrated_posterior"], serde_json::json!(false));
        assert_eq!(v["is_simulation_output"], serde_json::json!(true));
        assert_eq!(v["kind"], serde_json::json!("observation_frame"));
    }

    #[test]
    fn network_ref_field_is_ref_on_the_wire() {
        let n = NetworkRef {
            ref_: "iaf_psc_alpha".into(),
            ..Default::default()
        };
        let v: serde_json::Value = serde_json::to_value(&n).unwrap();
        assert_eq!(v["ref"], serde_json::json!("iaf_psc_alpha"));
        assert_eq!(v["kind"], serde_json::json!("builtin"));
    }

    #[test]
    fn version_guard() {
        // Wire is pre-1.0 (0.1), so the minor is breaking: an exact (major,
        // minor) match is required and a same-major/different-minor is rejected.
        assert!(check_version("0.1", true).unwrap()); // exact match ok
        assert!(check_version("0.9", true).is_err()); // 0.x minor diff is breaking -> Err under strict
        assert!(!check_version("0.9", false).unwrap()); // ...and Ok(false) when lenient
        assert!(!check_version("1.0", false).unwrap()); // different major incompatible
        assert!(check_version("1.0", true).is_err());
        assert!(check_version("bogus", false).is_err());
    }

    #[test]
    fn codec_encode_decode_roundtrip() {
        let codec = default_uav_velocity_codec();
        let mut channels = Map::new();
        channels.insert(
            "pose_error".into(),
            ChannelValue::vec3(2.0, 0.0, -2.0, Some("m")),
        );
        let sensor = SensorFrame {
            channels,
            ..Default::default()
        };
        let rates = codec.encode(Some(&sensor));
        // +2.0 error -> top of rate range; -2.0 -> bottom.
        assert!((rates["err_x"] - 200.0).abs() < 1e-6);
        assert!((rates["err_z"] - 0.0).abs() < 1e-6);
        let cmd = codec.decode(&rates, 0.0, 0, "world", Mode::Active);
        assert_eq!(cmd.channels["velocity_setpoint"].data.len(), 3);
    }

    /// A non-finite sensor sample must not poison the rate pipeline: a NaN error
    /// component encodes to the low bound of the rate range (fail-safe), never
    /// to a NaN rate.
    #[test]
    fn codec_nan_sensor_fails_safe_to_low_bound() {
        let codec = default_uav_velocity_codec();
        let mut channels = Map::new();
        channels.insert(
            "pose_error".into(),
            ChannelValue::vec3(f64::NAN, f64::INFINITY, f64::NEG_INFINITY, Some("m")),
        );
        let sensor = SensorFrame {
            channels,
            ..Default::default()
        };
        let rates = codec.encode(Some(&sensor));
        for axis in ["err_x", "err_y", "err_z"] {
            let r = rates[axis];
            assert!(r.is_finite(), "{axis} rate must be finite, got {r}");
            // rate_range_hz low bound is 0.0 for the default codec.
            assert!(
                (r - 0.0).abs() < 1e-9,
                "{axis} should fail safe to low bound, got {r}"
            );
        }
    }

    /// `validate()` must be honest: a `step_request` missing its required
    /// `session_id` is rejected even though the typed `serde` round-trip would
    /// silently default it to an empty string.
    #[test]
    fn validate_rejects_missing_required() {
        // Missing required `session_id`.
        let bad = serde_json::json!({"kind": "step_request", "advance_ms": 1.0});
        assert!(
            validate(&bad).is_err(),
            "missing session_id must be rejected"
        );
        // ...yet the typed round-trip happily defaults it (the bug validate closes).
        let typed: StepRequest = serde_json::from_value(bad).unwrap();
        assert_eq!(typed.session_id, "");

        // A complete step_request passes.
        let good = serde_json::json!({"kind": "step_request", "session_id": "s1"});
        assert!(validate(&good).is_ok());

        // Unknown kinds and non-objects are rejected.
        assert!(validate(&serde_json::json!({"kind": "not_a_real_kind"})).is_err());
        assert!(validate(&serde_json::json!([1, 2, 3])).is_err());
        assert!(
            validate(&serde_json::json!({"session_id": "s1"})).is_err(),
            "no kind -> err"
        );

        // Forward-compatible: unknown extra fields are still accepted.
        let fwd = serde_json::json!({"kind": "step_request", "session_id": "s1", "future": 7});
        assert!(validate(&fwd).is_ok());
    }
}
