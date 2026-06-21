# ncp-core

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **normative Rust reference** for the Neuro-Cybernetic Protocol (NCP): a versioned,
transport-agnostic, project-agnostic standard for letting a NEST-based simulation
serve external robot / UAV / simulation systems — for perception, action, both, or neither.

This crate is the one other NCP peers depend on. It is serde-only (no transport): the wire
types, the version guard (`NCP_VERSION` / `check_version`), the key scheme, a reference rate
codec, the action-plane safety governor, and an in-process bus and control loop. The Zenoh
transport lives in `ncp-zenoh`; the Python, TypeScript, and C++ peers (`ncp-python`,
`ncp-ts`, `ncp-cpp`) serialize to semantically-equivalent JSON, so all peers interoperate.

**Scientific boundary (binding):** returned `V_m`/spikes are raw simulation outputs of a
specified model, never a validated reproduction. Every `ObservationFrame` carries
`calibrated_posterior=false` and `is_simulation_output=true`. A neuro-controller is a control
artifact, never a paper-reproduction claim.

```rust
use ncp_core::{OpenSession, NetworkRef, NetworkRefKind, RecordSpec, RecordTarget, Observable};

let open = OpenSession {
    session_id: "uav3-percept".into(),
    network: NetworkRef {
        kind: NetworkRefKind::Builtin,
        ref_: "iaf_psc_alpha".into(),
        population_sizes: [("feat".to_string(), 1)].into_iter().collect(),
        ..Default::default()
    },
    record: RecordSpec { targets: vec![RecordTarget {
        port: "spk".into(), target: "feat".into(), observable: Observable::Spikes,
        ..Default::default()
    }] },
    ..Default::default()
};
let json = serde_json::to_string(&open).unwrap();
assert!(json.contains("\"kind\":\"open_session\""));
```

Public modules: `messages` (wire types + `validate`), `codec`, `keys`, `safety`, `bus`,
`bulk`, `resilience`, `transport`.

See the normative specification [`NEURO_CYBERNETIC_PROTOCOL.md`](../NEURO_CYBERNETIC_PROTOCOL.md)
and the [repository README](../README.md) for the full protocol, the wire contract, and the
peer matrix.

## License

Licensed under either of [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE) at your option.
