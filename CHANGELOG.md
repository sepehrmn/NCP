# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- `README.md` bibtex citation example pinned a stale `version = {0.2.7}`; corrected to
  `0.2.8` to match `CITATION.cff` and the crate/package version.

### Changed

- `scripts/check-version-coherence.sh` now also extracts and verifies the `README.md`
  bibtex `version = {…}` against the Cargo/npm/CITATION versions, so a stale citation
  example fails the guard (this was the exact drift that left the bibtex at `0.2.7`).

## [0.2.8] - 2026-06-20

### Security

- `ncp-zenoh`: secure-by-default transport. `ZenohBus::open`/`open_realm` now open a
  **hardened default** config with multicast scouting disabled, so a default
  deployment no longer auto-advertises on the LAN (peers still connect via explicit
  `connect`/`listen` endpoints). Added `ZenohBus::open_secure`,
  `ZenohBus::with_config_file`, and the `NCP_ZENOH_CONFIG` env hook (honored by
  `open`/`open_realm` and the `ncp-gateway` binary) to load the shipped per-plane ACL
  config (`deploy/zenoh-access-control.json5`). Loading fails closed — a missing or
  malformed config aborts the open rather than falling back to an open default, and
  `open_secure` refuses to start when `NCP_ZENOH_CONFIG` is unset. Documented that the
  realm is *addressing, not a credential*. No wire/proto change.

## [0.2.7] - 2026-06-20

Release-coherence fix. **No wire change** — `ncp_version` stays `0.2` and the conformance vectors are
unchanged. v0.2.6 was tagged but its crate manifests and the `@sepehrmn/ncp` npm package still
self-identified as `0.2.5` (the manifest version bump was omitted), so a consumer pinning `tag=v0.2.6`
compiled a crate reporting `0.2.5`. This release bumps the workspace crates and the npm package to
`0.2.7` so the git tag, the Cargo manifests, and the npm package agree. Consumers should re-pin from
v0.2.6 to v0.2.7. (v0.2.6 is left intact — tags are immutable once consumed.)

## [0.2.6] - 2026-06-20

Rebrand-only release. **No wire shape change** — `ncp_version` stays `"0.2"` and the JSON/binary
vectors are unchanged. **Compat note:** `CONTRACT_HASH` rebumped (`4c31db5c8eafbcf7` →
`07f829cabbd1684a`) because the proto's issue-reference comment changed; peers that exchange the
contract hash in `negotiate()` must run the same release, so upgrade the fleet together — this is
the designed contract-revision signal, not a wire break.

- Repointed all repository URLs from `github.com/sepehrmn/NCP` to
  `github.com/sepahead/NCP` (GitHub account rename); the `@sepehrmn/ncp` npm
  package name is unchanged (it is the published identity pinned by consumers).
  The proto's issue-reference comment changed too, so `CONTRACT_HASH` rebumped
  (`4c31db5c8eafbcf7` → `07f829cabbd1684a`); no wire/field/enum change.

## [0.2.5] - 2026-06-20

Conformance, validation, versioning, and supply-chain hardening — the v0.2.4
follow-on found by a 20-lens review. **No wire shape change** — `ncp_version` stays
`"0.2"` and the JSON/binary vectors are unchanged. **Compat note:** the proto's
version-policy comments were corrected (no field/enum/wire change), which rebumps
`CONTRACT_HASH` (`c35c4897a317049f` → `4c31db5c8eafbcf7`). Peers that exchange the
contract hash in `negotiate()` must run the same release, so upgrade the fleet
together — this is the designed contract-revision signal, not a wire break.

### Fixed
- **Conformance validator is now honest.** `check_conformance_vectors.py` no longer
  short-circuits `anyOf` (every nullable field — units, seed, durations, recordable,
  provenance — was previously unchecked) and gained primitive `type` checks, so a
  `{"type":"null"}` branch actually rejects a non-null and wrong-typed scalars fail.
- **Two-way proto↔schema parity.** `check_proto_schema_parity.py` added a reverse
  pass: a proto message with no JSON Schema (e.g. `BulkObservation`) and no
  documented allowlist entry now fails, closing the schema-only blind spot.
- **Language bindings validate like the reference.** `ncp-python` and `ncp-cpp`
  `validate()` previously only did a typed serde round-trip (silently defaulting a
  missing required field, round-tripping a tampered discriminator clean); they now
  delegate to `ncp_core::validate` first, and `link_status` was added to both
  dispatch tables (the one wire kind they were missing).
- **Version policy text matched the code.** The proto comments and the spec /
  VERSIONING docs said receivers check the "major only"; corrected to the actual
  exact `(major, minor)` pre-1.0 fail-closed rule (the README was already right).

### Added
- **`validate()` pins the scientific-boundary discriminators.** A frame asserting
  `calibrated_posterior=true` or `is_simulation_output=false` (top-level on
  `observation_frame`, or in `session_opened.provenance`) is now rejected, not
  trusted — mirroring the proto "always false"/"always true" contract.
- **Corpus coverage 4 → 13 kinds** with a coverage gate (every schema `kind` must
  have a golden vector), a `required_fields()`↔schema drift test, and a
  cross-language `ncp-cpp` corpus test that drives every JSON vector through the
  C ABI.
- **Supply-chain gate:** `cargo-deny` (advisories / licenses / bans / sources) +
  `deny.toml`, `--locked` on all CI cargo steps and the release publish dry-runs, a
  release tag↔version guard, and a `cargo check` of `ncp-python` (was never compiled
  in CI). The local `check.sh` now runs the parity + conformance gates too.

### Security
- **Glob subscribes enforce `check_id`** and the client `open()` runs the version
  handshake (carried over from the v0.2.4 transport work). `cargo-deny` documents
  three transitive advisories pinned by `zenoh 1.9.0` (lz4_flex block-API OOB,
  `paste`/`rustls-pemfile` unmaintained) with remove-when conditions.

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

[Unreleased]: https://github.com/sepahead/NCP/compare/v0.2.8...HEAD
[0.2.8]: https://github.com/sepahead/NCP/compare/v0.2.7...v0.2.8
[0.2.7]: https://github.com/sepahead/NCP/compare/v0.2.6...v0.2.7
[0.2.6]: https://github.com/sepahead/NCP/compare/v0.2.5...v0.2.6
[0.2.5]: https://github.com/sepahead/NCP/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/sepahead/NCP/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/sepahead/NCP/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/sepahead/NCP/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/sepahead/NCP/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/sepahead/NCP/releases/tag/v0.2.0
