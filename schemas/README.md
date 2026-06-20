# NCP JSON Schemas

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

JSON-Schema (draft 2020-12) mirrors of the NCP message types — the **JSON projection** of the
normative protocol, used to validate the JSON transport and the conformance corpus.

NCP is one normative protocol with peers in Rust (`ncp-core`), Python (`ncp-python`), TypeScript
(`ncp-ts`), and C++ (`ncp-cpp`). The wire contract lives in [`proto/ncp.proto`](../proto/ncp.proto);
the schemas here are one of the three wire projections (Rust serde / JSON Schema / protobuf) that a
parity guard keeps from drifting apart.

## Source of truth & regeneration (IMPORTANT)

These files are **generated, not hand-edited**. In the engram deployment they are emitted from the
Paper2Brain Pydantic models via `backend/neurocontrol/export_schemas.py`, and a drift guard checks
the committed copy against a fresh export. See the `note` in [`index.json`](index.json):

```text
"Generated from backend/neurocontrol Pydantic models; do not edit by hand."
```

Do not edit a `*.schema.json` by hand — regenerate from the Pydantic source and commit the result.
[`index.json`](index.json) lists the message set (`ncp_version` `0.2`): `capabilities`,
`open_session` / `session_opened`, `close_session` / `session_closed`, `run_request`,
`step_request`, `sensor_frame` / `stimulus_frame` / `observation_frame` / `command_frame`,
`control_status`, `link_status`.

## Drift guards

- [`scripts/check_proto_schema_parity.py`](../scripts/check_proto_schema_parity.py) — field-set and
  enum wire-string parity between `proto/ncp.proto` and `schemas/*.schema.json`.
- `ncp-core/tests/conformance.rs` — guards the Rust serde types against these schemas.

## See also

- [`NEURO_CYBERNETIC_PROTOCOL.md`](../NEURO_CYBERNETIC_PROTOCOL.md) — the human-readable spec.
- [`proto/ncp.proto`](../proto/ncp.proto) — the normative wire contract.
- [repository README](../README.md) — the full SDK overview.

## License

Licensed under either of [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE) at your option.
