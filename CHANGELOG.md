# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.4] - 2026-06-20

Safety, validation, and security hardening — the remaining major findings from the
10-lens protocol audit after v0.2.3. No wire change — `ncp_version` stays `"0.2"`;
all changes are plant-side/behavioral fixes, fail-safe deserialization, receive-path
validation, a sensor-plane ACL invariant, observability, and normative docs, so
existing peers and conformance vectors are unaffected. Crates/package `0.2.4`.

### Fixed
- **Real-time: `ActionBuffer`/`CommandWatchdog` reject stale & reordered commands.**
  A duplicate/reordered/replayed `CommandFrame` could overwrite a newer one and
  rewind the replay clock, and a trickle of stale commands kept the watchdog
  deadline "fresh" (fail-open during a blackout). Now: monotonic-forward `seq`
  acceptance drops them (`seq == 0` escape hatch for pull/sim streams) and the
  watchdog refreshes only on a strictly-advancing `seq`; an ESTOP still latches even
  if stale.
- **Safety: an unknown `mode` string deserializes to `HOLD`** rather than
  hard-erroring the whole `CommandFrame` (complements the v0.2.3 absent-mode→HOLD).
- **Resilience: bulk parallel columns must agree in length.** `observation_from_bulk`
  rejects a block whose `times`/`values`/`senders` disagree — fail closed at the
  untrusted-bytes boundary instead of silently pairing mismatched arrays.
- **Interop/safety: wrong-`kind` RPC replies are rejected** before the typed decode,
  so a misrouted but valid-JSON reply no longer becomes an all-default response.

### Security
- **Sensor-plane PUT is access-controlled, symmetric to the command plane.** The
  perception plane is a control input — a spoofed `SensorFrame` steers the controller
  and defeats the geofence (false-data injection) — so `check_acl_template.py` now
  enforces sensor-PUT → `robot` (and self-tests every run), and `SECURITY.md`
  documents the threat + remedy (publisher access control per DDS-Security / SROS2).

### Added
- `diagnose_version()` + a sensor-subscriber diagnostic so a dropped,
  version-incompatible frame is observable rather than silently ignored.

### Documentation
- A **normative action-plane liveness conformance clause**: a plant **MUST** fail
  safe (HOLD) on expired `ttl_ms` and **MUST NOT** actuate on a stale setpoint (the
  wire only detects a gap; the plant owns the safe state — RFC 2119/8174;
  IEC 61508 / ISO 13849 framing).

## [0.2.3] - 2026-06-20

Contract-vs-implementation reconciliations from a 10-lens protocol soundness audit.
No wire change — `ncp_version` stays `"0.2"`; the changes are a behavioral safety
fix, a fail-safe deserialization default, and doc corrections, so existing peers
and conformance vectors are unaffected. Crates/package `0.2.3`.

### Fixed
- **Real-time: `CommandFrame.seq` now echoes the originating `SensorFrame.seq`.**
  `NeuroControlLoop::tick()` overwrote it with the loop's own free-running counter,
  breaking the normative split-plane V↔A join (an observer pairing action to sensor
  on `seq` would mispair). The loop's tick counter now lives only on `ControlStatus`.
- **Safety: a `CommandFrame` that omits `mode` now deserializes to `HOLD`, not
  `ACTIVE`.** An untrusted/partial wire frame must never default to actuating; added
  a fail-safe serde field default. Programmatic `CommandFrame::default()` is unchanged.

### Documentation
- **Transport QoS corrected to match the Zenoh binding.** It sets best-effort
  `CongestionControl::Drop` + priority + `express` only — NOT conflation/keep-last,
  reliability, or a wire TTL/`LIFESPAN`; `ttl_ms` is enforced plant-side by
  `CommandWatchdog`. Fixed the key scheme, the DDS-mapping table (now labelled
  "DDS mapping, not set today"), README, and RESILIENCE.
- **Versioning policy clarified.** `check_version`/`negotiate` are fail-closed
  *library* entry points, not yet auto-invoked on the data-plane receive path;
  per-session `open_session` negotiation remains a ROADMAP P1 target.

## [0.2.2] - 2026-06-19

Hardening pass against ROADMAP P0/P1/P2 and a full-repo review. No wire change —
`ncp_version` stays `"0.2"`; all additions are additive APIs, a config fix, docs,
and CI guards, so existing peers/vectors are unaffected. Crates/package `0.2.2`.

### Security
- **P0 / #7 — the per-plane ACL template now actually loads.** `deploy/zenoh-access-control.json5`
  used `"get"` in `messages`, which is not a valid Zenoh access-control token, so
  zenohd would reject the whole config — leaving the world-writable action plane
  with no mitigation while reading as "secured." Replaced with the correct tokens
  (`query` for the get/querier side; pure data-plane reads use `declare_subscriber`)
  and clarified that `cert_common_names` matches by **exact** string (not glob).
  Added `scripts/check_acl_template.py` (stdlib-only) as a CI guard: it fails on any
  invalid `messages` token and enforces the safety invariant that only the `engram`
  commander policy may PUT on `…/command/**`. Wired into `scripts/check.sh`.

### Fixed
- **Safety (critical): predictive horizon bypassed the speed clamp.** `SafetyGovernor::govern`
  clamped only the tick-0 command; `CommandFrame.horizon[1..]` — replayed verbatim
  by `ActionBuffer` through dropouts — passed unclamped, defeating `max_speed_mps`
  on every tick after 0. Now every horizon step is speed-clamped with the same
  logic, and a step that cannot be enforced (absent/non-finite velocity) truncates
  the horizon there so replay HOLDs rather than emitting an unbounded setpoint.
- **Safety: staleness/watchdog backstops failed OPEN on a non-finite clock.** A NaN
  `now_s` made `(now_s - last) > timeout` evaluate false ("fresh") in both
  `SafetyGovernor::govern` and `CommandWatchdog::should_hold`; both now treat any
  non-finite clock input as stale/expired (HOLD).
- **Versioning: `check_version` coerced a malformed minor to 0 (latent fail-open).**
  A present-but-garbage minor (`"0.GARBAGE"`) or extra component (`"0.2.1"`) silently
  parsed to a `0` minor; minor parsing is now as strict as major (reject non-numeric
  / trailing components), so the fail-closed guard cannot become fail-open.
- **Codec: a dropped readout population decoded to full-reverse actuation.**
  `CodecSpec::decode` mapped an absent population to the value-range low bound
  (e.g. −1.5 m/s — commanded motion the governor's magnitude clamp passes). It now
  maps to the documented neutral midpoint (0.0 for a symmetric range).
- **Resilience: reordering permanently inflated `loss_rate`.** `LinkMonitor` counted
  a later-arriving (merely reordered) seq as a permanent loss, spuriously escalating
  the burst/HOLD fail-safe. It now reconciles out-of-order arrivals against a bounded
  missing-seq set, decrementing `lost`. `LinkMonitor::new` also validates/clamps
  `ref_loss` (to `[0,0.99]`) and `threshold` (>0, finite) so an out-of-range param
  can no longer disable or false-trip the jam detector.

### Added
- **P1 — wire-contract pinning.** `ncp_core::CONTRACT_HASH` (FNV-1a of `proto/ncp.proto`),
  `fnv1a_hex`, `verify_contract`, and `negotiate(version, contract_hash)` — a single
  "negotiate, reject, never coerce" handshake gate that detects a post-agreement
  schema mutation (the "rug-pull" class). A conformance test recomputes the hash from
  the real proto, so a proto edit that forgets to bump the constant fails CI.

### Changed
- **P2 — dual licensing.** Moved to `MIT OR Apache-2.0` (the Rust-ecosystem norm):
  `LICENSE` → `LICENSE-MIT`, added `LICENSE-APACHE`, and updated `Cargo.toml`,
  `package.json`, `CITATION.cff`, and the README license section/badge.

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

[Unreleased]: https://github.com/sepehrmn/NCP/compare/v0.2.4...HEAD
[0.2.4]: https://github.com/sepehrmn/NCP/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/sepehrmn/NCP/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/sepehrmn/NCP/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/sepehrmn/NCP/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/sepehrmn/NCP/releases/tag/v0.2.0
