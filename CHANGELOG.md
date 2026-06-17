# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1] - 2026-06-17

Patch release: no wire change (`ncp_version` stays `"0.2"`); fixes a shipped-artifact
defect, doc accuracy, and documentation consistency. Crates/package versioned `0.2.1`.

### Fixed
- **The shipped TS package announced the wrong wire version.** The git-tracked,
  published `ncp-ts/dist` (the `@sepehrmn/ncp` build artifact) still stamped
  `ncp_version "0.1"` after the `0.2` source bump — so a JS/TS peer would be
  version-rejected by the Rust/Python peers. Rebuilt `dist` to `"0.2"`, pinned
  `typescript` for a reproducible build, and added a `ts dist up-to-date` CI job
  that fails when `dist` drifts from source.
- Doc accuracy in `ncp-core::bulk`: `Column::as_f64`/`as_i64` now note the lossy
  (i64→f64 >2^53 / float→int) arms not exercised by the codec; `BulkBlock::encode`
  documents its size limits; the `ncp-cpp` version-doc example says `"0.2"`.

### Changed
- Documentation consistency sweep across the markdown set (version strings, MSRV
  1.88, `v0.2.x` feature coverage for the bulk codec / ACL / governance / neuron
  families, and cross-link integrity).

## [0.2.0] - 2026-06-17

Pre-1.0 / pre-release: the wire contract may still change. The crates are versioned
`0.2.0` in `Cargo.toml` and the wire `ncp_version` string is `"0.2"`; tagged
`v0.2.0` (the first proto-bearing baseline, used by the `buf breaking` gate). The
`0.1`→`0.2` changes are additive, but a pre-1.0 minor bump is fail-closed by the
version guard, so peers must speak `0.2`.

### Added
- Initial protocol + Rust reference SDK (`ncp-core`, `ncp-zenoh`, `ncp-gateway`,
  `ncp-python`, `ncp-cpp`): QoS-differentiated Zenoh transport, a safety-gated
  action plane (mode/`ttl_ms`, latched ESTOP, fail-closed watchdog/geofence), and
  per-frame provenance.
- Two wire conformance guards: `ncp-core/tests/conformance.rs` (Rust serde ↔ JSON
  Schema) and `scripts/check_proto_schema_parity.py` (`proto/ncp.proto` ↔ JSON
  Schema — field-set + enum wire-string parity), both in CI.
- Buf scaffold (`buf.yaml` / `buf.gen.yaml`): lint, build and polyglot codegen
  (Rust/TS/Python) from `proto/ncp.proto`; `buf lint` in CI.
- **Neuron-family coverage (#10):** generic named-recordable + named-param wire —
  `Observable.binary_state`, `StimulusKind.rate_inject`, `RecordTarget.recordables`,
  `Observation.recordable`, `StimulusTarget.params` — so the contract serves NEST's
  point/conductance (`g_ex`/`g_in`/`w`), binary, and rate-based neuron families, not
  just spiking. Additive; the Engram reference backend wires it to NEST 3.9
  (`multimeter`/`step_rate_generator`/`spin_detector`), verified live.
- `VERSIONING.md` (SemVer wire policy + buf-breaking enforcement + version-negotiation
  target), a golden-vector **conformance corpus** (`conformance/vectors/` +
  `scripts/check_conformance_vectors.py`, in CI), and `deploy/zenoh-access-control.json5`
  (per-plane ACL template).
- **Bulk column codec (#6):** `ncp-core::bulk` — a packed little-endian, parse-free,
  random-access column block (`f32`/`f64`/`i32`/`i64`) for bulk observation arrays
  (spike trains, V_m traces), with the `BulkObservation` proto envelope. Additive,
  observation-plane-only (never the hot action loop); fully bounds-checked decode of
  untrusted bytes. A binary golden vector (`conformance/vectors/bulk_observation.bin`)
  + a Python reference decoder make it cross-language conformance-checked, byte-pinned
  to the Rust encoder.
- **Conformance corpus now spans JSON *and* binary (#9):** the validator checks the
  bulk binary vector via a stdlib reference decoder; `GOVERNANCE.md` documents the
  governance model + neutral-home path.
- **Action-plane authentication (#7):** corrected and completed the per-plane Zenoh
  ACL template into a functional default-deny policy (distinct engram/robot/observer
  subjects; only `engram` may publish commands; clients may query the RPC), and added
  concrete TLS+ACL enablement steps to `SECURITY.md` (DDS-Security / MAVLink-2-signing
  comparators already documented).
- `@sepehrmn/ncp` TypeScript package (`ncp-ts`): generated wire types, a
  transport-agnostic `NeuroSimClient`, and a WebSocket transport. The client
  surfaces server `{kind:"error"}` frames as thrown errors and rejects unsafe
  (>2^53) seeds.
- Release scaffolding (LICENSE, CITATION.cff, SECURITY.md, ROADMAP.md, this
  changelog, crates.io metadata) and CI.

### Changed
- **proto-native:** promoted `proto/ncp.proto` to the **normative wire contract**
  (previously a non-normative mirror). The JSON Schemas are its JSON projection and
  `ncp-core` is the reference implementation; all docs reconciled to this model.
- Named the protocol the **Neuro-Cybernetic Protocol (NCP)**.
- Vendored the spec, `.proto` definitions, and JSON schemas into the SDK so the
  wire contract ships with the reference implementation rather than living out of
  tree.

### Fixed
- **CI was red on every push and PR.** Transitive deps in `Cargo.lock` (`darling`,
  `time`/`time-core`, `rcgen`, `serde_with`, `home`) declare `rust-version 1.88`,
  and edition2024 deps need ≥1.85 — the pinned `1.81.0` toolchain could not even
  parse their manifests. Bumped the MSRV / CI toolchain to **1.88.0** (`Cargo.toml`,
  `ci.yml`, `release.yml`, README badge), unblocking the fmt/clippy/test gate and
  the dependabot dependency PRs.

[Unreleased]: https://github.com/sepehrmn/NCP/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/sepehrmn/NCP/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/sepehrmn/NCP/releases/tag/v0.2.0
