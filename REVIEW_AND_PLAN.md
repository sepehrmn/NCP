# NCP (Neuro-Cybernetic Protocol) — Full-Repo Review

## 1. Overall assessment

This is a carefully engineered, unusually honest repo. The wire-contract discipline (proto ↔ schema ↔ Rust ↔ ts-rs parity, conformance corpus, exact enum wire-strings, forward-compat handling) and the safety-governor design (latched ESTOP, fail-closed timeout edge cases, NaN handling) are genuinely strong, and the docs are refreshingly self-critical (RATIONALE.md, VERSIONING.md, the ncp-zenoh module header). The core problems are not sloppiness but **gaps where the guards stop short of their own advertised guarantees**: two critical safety/security holes (unclamped predictive horizon, a non-loading ACL template), several fail-open seams in safety/version/codec/resilience code, and parity guards that miss field types, field numbers, and enum-constant mapping. None of the critical/high findings are hard to fix; most are "tighten the guard so it actually covers what the doc claims."

---

## 2. Findings by severity

### Critical

| id | title | location | impact | fix |
|----|-------|----------|--------|-----|
| safety-1 | Predictive horizon setpoints bypass geofence and speed clamp | `ncp-core/src/safety.rs:286-317` | `govern()` clamps only tick-0 channels; `horizon[1..]` passes verbatim and is replayed by ActionBuffer, defeating speed limit + geofence on every tick after 0 | Clamp/evaluate every horizon step with the same speed+geofence logic, or HOLD/reject any horizon containing an over-limit step |
| security-1 | ACL template uses invalid Zenoh token `"get"` — config won't parse | `deploy/zenoh-access-control.json5:35` | zenohd refuses the config (or ACL silently doesn't load); the one concrete mitigation for the world-writable action plane is non-functional as shipped | Use `query`/`declare_subscriber` per plane; add a CI check that the json5 deserializes against `zenoh::Config` |

### High

| id | title | location | impact | fix |
|----|-------|----------|--------|-----|
| core-wire-1 | `check_version` coerces malformed minor to 0 — latent fail-open | `ncp-core/src/messages.rs:821-827` | Once `NCP_VERSION` minor is 0 (e.g. "1.0"), any garbage minor ("2.GARBAGE") parses to (2,0) and is accepted by a fail-closed guard | Make minor parsing as strict as major; reject >2 components / trailing junk |
| safety-2 | Staleness check and `CommandWatchdog` fail OPEN on NaN clock | `ncp-core/src/safety.rs:247-255, 347-352` | NaN `now_s` makes `NaN > timeout` false → `stale=false` → live command passes; a safety backstop fails open on a bad clock | Treat `!now_s.is_finite()` (and non-finite `last`) as stale / should_hold |
| codec-bus-1 | `decode()` of a missing population yields full-negative velocity | `ncp-core/src/codec.rs:154-161, 202-210` | A dropped/renamed readout lerps to `-1.5 m/s` (commanded motion), not neutral; governor only clamps magnitude so it passes | Skip absent populations or map to a documented neutral; test that a missing pop never yields max-magnitude actuation |
| python-1 | `validate()` bypasses `ncp_core::validate`, accepts malformed frames | `ncp-python/src/lib.rs:159-185` | Python `validate()` accepts frames missing required fields the Rust peer rejects; defeats the canonical-validator routing | Parse to `serde_json::Value` and call `ncp_core::validate`; drop the kind-dispatch macro |
| ts-client-1 | Failed `open_session` silently treated as success (fail-open) | `ncp-ts/src/client.ts:85-90, 119` | No `error` kind exists in the contract; failure arrives as `session_opened{ok:false}`, `unwrap` only throws on `kind==='error'`, so callers proceed against an unopened session | After unwrap, throw on `reply.ok===false`; validate `SessionClosed.ok`; add a regression test |
| parity-2 | Removing ALL wire-string annotations downgrades enum check to a note | `scripts/check_proto_schema_parity.py:168-176` | The exact mechanism the guard exists to catch (deleting annotations) makes CI pass — fail-open | Make a fully-unannotated real wire enum a hard FAILURE; allow notes only for an explicit string-modeled allow-list |
| parity-3 | Guard checks only field-NAME sets — type and field-number drift pass | `scripts/check_proto_schema_parity.py:147-155` | Type changes (double↔int64) and field-number re-tags silently break the binary wire while JSON peers are unaffected; contradicts "no representation can silently diverge" | Capture `(name, type, number)`; cross-check type vs schema JSON type and field numbers vs a lockfile |
| security-3 | Default-deny ACL has no plant subject — breaks the control loop | `deploy/zenoh-access-control.json5:54-65` | Applying the ACL denies the plant command-subscribe and sensor-publish, killing actuation and perception | Add a `plant` subject (`put` sensor/**, `declare_subscriber` command/**); make all four roles explicit |
| build-ci-1 | Published TS package unusable under native ESM | `ncp-ts/tsconfig.json:7` (`moduleResolution:"Bundler"`) + `ncp-ts/dist/index.js:11-12` | Extensionless relative imports throw `ERR_MODULE_NOT_FOUND` for the exact documented Node import; bundlers masked it | Emit `.js` extensions + `module/moduleResolution: NodeNext`; CI smoke test: `npm pack` → install → `node import()` |
| docs-eng-1 | `bench_realtime.py` computes `fire_hz` cumulatively (~3x inflated) | `scripts/bench_realtime.py:123-136` | `n_events` is cumulative across reps but divided by one rep's bio time; at default reps=3, fire_hz ~3x high, breaking the docs' correctness check | Capture `n0` before a single timed rep (or per-rep); fix the doc text |

### Medium

| id | title | location | impact | fix |
|----|-------|----------|--------|-----|
| core-wire-2 | `validate()` checks only top-level required, not nested | `ncp-core/src/messages.rs:906-922` | `network:{}` passes despite missing schema-required `ref`; serde defaults it to `""`, defeating validate's stated purpose | Recurse into `$defs` required arrays, or downgrade the docstring to "top-level only" |
| safety-3 | `config_fail_closed` skipped when `sensor_channels` is empty | `ncp-core/src/safety.rs:111-113, 125-151` | An active geofence with empty sensor set is structurally unenforceable yet reports `safety_ok=true` | Drop the `!is_empty()` guard; `geofence_radius>0 && sensor_channels.is_empty()` ⇒ fail closed |
| codec-bus-2 | `MAX_COMPONENT` doesn't bound total allocation | `ncp-core/src/codec.rs:147-166` | Untrusted `CodecSpec` with N decoder entries each at component=4095 allocates N×4096×8 bytes; comment overstates the DoS defense | Bound total entry count / summed buffer size on deserialize, or soften the comment |
| resilience-1 | Out-of-order delivery permanently inflates `loss_rate` | `ncp-core/src/resilience.rs:150-164` | Reordering (0,1,4,2,3 → loss_rate 0.286 at zero real loss) gates the p_c HOLD→ESTOP fail-safe, spuriously escalating | Decrement `lost` when a gap-counted seq arrives, or compute loss over a sliding window |
| resilience-2 | No validation of `ref_loss`/`threshold` — jam detector fail-open | `ncp-core/src/resilience.rs:116-128, 135-139` | `ref_loss>=1.0` makes burst never trip (the load-bearing jam trigger); `threshold<=0` false-trips; NaN poisons | Validate in `new()`: `0<=ref_loss<1`, `threshold>0`, both finite (clamp or Result) |
| zenoh-1 | Docs advertise QoS (conflate-to-latest, reliable, KEEP_LAST(1)) code doesn't set | `README.md:44-46`, `NEURO_CYBERNETIC_PROTOCOL.md:211,229-231` vs `ncp-zenoh/src/lib.rs:50-67,345-353` | Consumers get `CongestionControl::Drop`/`Block`, not conflation/reliability; wire-contract drift | Implement via the advanced API, or align the spec table to actual Drop/Block semantics |
| zenoh-4 | `loopback.rs` is the only integration test; RPC/error/fan-out/QoS untested | `ncp-zenoh/tests/loopback.rs` (whole file) | The contract-critical `serve_rpc` round trip and error semantics have zero runtime coverage | Add serve_rpc round-trip (incl. error frame), named/glob fan-out, and fleet tests |
| python-2 | `validate()` ignores the embedded `kind` | `ncp-python/src/lib.rs:160-185` | Validates against caller-supplied kind, mislabelling frames; diverges from Rust | Fold into python-1 (derive kind from payload); optionally assert arg==payload kind |
| python-4 | Python binding has no tests / no working CI path | `.github/workflows/ci.yml:31-41`; `ncp-python/src/lib.rs` | Every Python bug above is invisible to CI; wire-identity claim unverified | Add `#[cfg(test)]` for wrapper logic + a maturin+pytest CI job round-tripping conformance vectors |
| python-5 | `govern()` silently disables the ESTOP latch | `ncp-python/src/lib.rs:129-151` | Python peer gets weaker safety than Rust: a breach un-latches the next in-fence frame, no supervisor reset | Expose a stateful `#[pyclass] Governor`; keep the one-shot only if renamed to make no-latch unmissable |
| python-7 | `Keys` missing `sensor_glob`/`command_glob`/`fleet_glob` | `ncp-python/src/lib.rs:42-72` | Python fleet/observer subscribers must hand-format wildcard keys, reintroducing divergence risk | Add the three glob methods delegating to CoreKeys |
| cpp-2 | No parity guard for `ncp.h` despite docs claiming a "cbindgen header" | `scripts/check.sh:28` | Hand-written header can silently drift from the `extern "C"` ABI → silent UB at C call sites; docs mislead maintainers | Add real cbindgen + CI diff, or compile-and-link a C TU; meanwhile fix the docs |
| parity-1 | Enum check is set-based — inverted constant→wire-string mapping passes | `scripts/check_proto_schema_parity.py:167-185` | Swapping which proto constant maps to which wire string corrupts every binary-wire enum value while CI stays green | Verify per-value mapping (ordered list / checked-in table), not just membership |
| parity-4 | Conformance validator short-circuits all anyOf/nullable fields | `scripts/check_conformance_vectors.py:46-48` | Every nullable wire field is completely unvalidated; malformed golden vectors pass | Actually evaluate anyOf branches; require one to validate; enforce unknown-field rejection on object branches |
| parity-5 | Proto-only messages/enums invisible to the guard | `scripts/check_proto_schema_parity.py:133-166` | A proto/ts-rs type with no schema (e.g. SetpointStep) drifts freely, uncovered | Add a reverse pass: every wire proto message must have a matching schema; cross-check index.json |
| parity-6 | `mode` typed as plain string in proto but enum in schema/Rust | `proto/ncp.proto:320,333` | A proto peer can send `mode:"banana"` — wire-valid in proto, rejected everywhere else, on a safety-critical authority field | Model `mode` as the `Mode` enum in proto (preferred), or assert the $ref + decoder-side membership check |
| security-2 | `cert_common_names` `.*` matched by exact equality, not glob | `deploy/zenoh-access-control.json5:55-57` | A real CN never matches `controller.*`; properly-identified controllers are denied (or all collapse to one literal CN) | Use exact per-role CNs, or document that Zenoh CN matching is exact-string |
| security-4 | Controller granted put/delete on perception plane (`sensor/**`) | `deploy/zenoh-access-control.json5:22-30` | A compromised controller can forge/delete sensor frames, feeding the governor false state; pure excess authority | Remove `sensor/**` from the write rule; controller reads sensors via the subscriber rule only |
| security-5 | SECURITY.md overstates the local fail-safe | `SECURITY.md:14-19` | No mode-based rejection exists; `ttl_ms` is enforced only if the plant wires `CommandWatchdog` (nothing in the SDK does) | Correct the doc to state what the governor actually enforces and where; mark CommandWatchdog as required plant-side glue |
| build-ci-2 | CI never builds/verifies the committed TS package | `.github/workflows/ci.yml` vs `scripts/check.sh:13-14`, `package.json:18-21` | Stale `dist/` and drifted `src/generated` can merge undetected, shipping broken JS (build-ci-1) | CI: `npm ci` → `npm run regen` → `git diff --exit-code` on generated + dist |
| build-ci-3 | Hand-maintained C ABI header has no codegen/parity guard | `ncp-cpp/include/ncp.h` + `ncp-cpp/Cargo.toml` + `scripts/check.sh:28` | Signature drift → UB/link errors at the FFI boundary with nothing catching it | Generate via cbindgen + CI diff, or compile a C TU against the cdylib/staticlib |
| docs-spec-1 | Spec §3 omits normative wire fields and two enum values | `NEURO_CYBERNETIC_PROTOCOL.md:108-124` | Spec under-describes the contract (missing `binary_state`, `rate_inject`, `recordables`, `params`, `recordable`) the README touts | List all 5 observables / 5 stimulus kinds and the named-recordable/param fields; note recordable-over-observable precedence |
| docs-spec-3 | Spec references nonexistent files + broken `ncp/` prefix | `NEURO_CYBERNETIC_PROTOCOL.md:24,35-36,48,162,257-293,300` | Dead links; presents Engram-repo Python components as shipping here | Strip the `ncp/` prefix; relink or remove dead refs; mark Python pieces as living in Engram |
| docs-eng-2 | Docs claim `fire_hz` is first-rep count but code uses cumulative | `NEST_REALTIME.md:221-222`, `PERFORMANCE.md:267-268` | The doc's justification masks the real ~3x inflation (docs-eng-1) | Fix code to per-rep count and keep the doc, or correct both |

### Low / Info

| id | title | location | impact | fix |
|----|-------|----------|--------|-----|
| core-wire-3 | Key builders enforce id validity only via `debug_assert!` | `ncp-core/src/keys.rs:58-92` | In release, an injected id (`s1/command/x`) is interpolated raw for non-Zenoh consumers — zero enforcement | Return Result / unconditional assert, or sanitize inside the builder; else narrow the docstring |
| core-wire-4 | Docstrings reference nonexistent `ncp/schemas/` path | `ncp-core/src/messages.rs:856,898` | Misdirects auditors; schemas live at repo-root `schemas/` *(unverified)* | Replace `ncp/schemas/` with `schemas/`; fix conformance.rs header |
| safety-4 | Fail-closed-to-HOLD paths report `safety_ok=true` | `ncp-core/src/safety.rs:166-171,268-271,291-294,298-301` | Supervisor can't distinguish "HOLD on purpose" from "safety unevaluable"; misleading green | Add a transient `last_unsafe` flag on these branches, or align the docstring |
| safety-7 | `from_capabilities` assumes FIRST channel is position/velocity | `ncp-core/src/safety.rs:119-151` | Mis-ordered channels make the geofence fence the wrong (e.g. IMU) channel; passes misconfig check *(unverified)* | Resolve position/velocity by explicit role/kind, not list order |
| codec-bus-3 | `LocalBus::query` returns first registration-order match | `ncp-core/src/bus.rs:81-92` | Overlapping queryables resolve silently by declare order *(unverified)* | Detect overlapping patterns at declare, or prefer most-specific match |
| codec-bus-4 | Encoder/decoder target collisions silently overwrite | `ncp-core/src/codec.rs:116-128,162-167` | Duplicate targets clobber (last wins); wrong wire result, no diagnostic *(unverified)* | Add `CodecSpec::validate()` rejecting duplicates |
| codec-bus-5 | `loop_latency_ms`/`sim_time_ms` never populated | `ncp-core/src/transport.rs:209-215` | Telemetry shows constant-zero loop latency *(unverified)* | Measure and populate, or document the binding fills it in |
| codec-bus-6 | `codec.rs` has no test module | `ncp-core/src/codec.rs` (whole file) | Decode-side risk branches (MAX_COMPONENT, missing-pop, lerp) unexercised *(unverified)* | Add decode-focused tests (esp. missing pop ≠ max magnitude) |
| resilience-3 | `horizon_dt_ms=Some(NaN/inf)` bypasses the legacy single-step guard | `ncp-core/src/resilience.rs:70-80` | `dt<=0.0` is false for NaN; horizon silently discarded (no replay) *(unverified)* | Use `!(dt > 0.0)`; reject non-finite `horizon_dt_ms` at ingest |
| zenoh-2 | Glob subscribe entry points bypass `check_id` | `ncp-zenoh/src/lib.rs:204-210,309-315` | A caller passing `session_id="*"` widens subscription to other sessions/planes (read-only info leak) | Add `check_id("session", session_id)?` to subscribe_session/subscribe_sensors_glob |
| zenoh-3 | `request()` returns first OK, ignores later error replies | `ncp-zenoh/src/lib.rs:159-166` | Multiple queryables on one realm → non-deterministic RPC result *(unverified)* | Document single-queryable assumption, or detect >1 OK |
| zenoh-5 | `keys.rs` doc says action plane is "reliable+TTL" — contradicts DROP QoS | `ncp-core/src/keys.rs:12` vs `ncp-zenoh/src/lib.rs:53,57-67` | In-repo docs disagree on reliability *(unverified)* | Update keys.rs table to express+DROP, TTL safety-gated by sender |
| gateway-5 | Gateway does no version negotiation/validation | `ncp-gateway/src/main.rs:32-80` | Incompatible-minor clients forwarded straight to nest.Run if Python doesn't gate *(unverified)* | Document the delegation, or parse `ncp_version` and reject before the bridge round-trip |
| gateway-6 | No tests for `forward_to_python` framing/error mapping | `ncp-gateway/src/main.rs` (no test module) | Framing/error-frame contract can regress silently *(unverified)* | Add fake-echo-server tests (CRLF, empty reply, connect-refused, empty request) |
| python-8 | `parse_mode` hand-duplicates Mode wire strings | `ncp-python/src/lib.rs:116-124` | Latent wire-drift if a Mode variant is added/renamed *(unverified)* | Deserialize through serde so the core enum is the single source |
| python-9 | maturin instructions / abi3 floor inconsistent | `ncp-python/src/lib.rs:10-14`; `ncp-python/Cargo.toml:23` | `maturin develop` doesn't work as written; abi3-py311 contradicts the 3.8+ goal *(unverified)* | Add the presupposed pyproject.toml; reconcile the abi3 floor |
| cpp-3 | `demo.cpp` never compiled/linked in CI | `scripts/check.sh:28-29` | C linkage / header includability / demo smoke asserts unvalidated by automation *(unverified)* | Add a CI step that builds+runs the demo and asserts zero exit |
| ts-client-5 | `duration_ms`/`advance_ms` not finiteness-guarded (seed is) | `ncp-ts/src/client.ts:106-108,132,153` | NaN/Infinity become JSON `null` → confusing "missing required field" *(unverified)* | Add `Number.isFinite` (and `>=0`) guards, mirroring the seed guard |
| parity-7 | Conformance validator does no type checking | `scripts/check_conformance_vectors.py:45-77` | A golden vector with the wrong primitive type is certified conformant *(unverified)* | Add scalar type enforcement (int/number/string/bool), note int64 precision caveat |
| parity-8 | Working-tree `gen/` stale vs proto | `gen/rust/...:441-489` etc. | Gitignored so no committed drift, but can mislead a developer inspecting it *(unverified)* | Regenerate in the check step or remove; note gen/ is preview-only |
| parity-9 | README overstates "no representation can silently diverge" | `README.md:56` | Field-set guards can't catch types/numbers/enum-mapping/proto-only/ts-rs drift *(unverified)* | Scope the claim to field-name + enum-wire-string parity |
| parity-10 | Conformance corpus covers 4/13 kinds, 2 enum strings | `conformance/vectors/`, `ncp-core/tests/conformance.rs:272,285` | Safety-critical Mode strings (estop!) and the capabilities handshake have no golden vector *(unverified)* | Add a vector per kind and per safety-critical enum value; assert coverage of index.json |
| security-6 | ACL comment references a nonexistent "stimulus plane" | `deploy/zenoh-access-control.json5:20-30` | Operator hunts for a key that doesn't exist; compounds role confusion *(unverified)* | Drop "stimulus plane"; describe command=action, sensor=perception accurately |
| security-7 | observer-reads grants `get`/query on a pure pub/sub plane | `deploy/zenoh-access-control.json5:34-42` | Dead authority that widens the rule beyond least-privilege *(unverified)* | Scope read rule to `["declare_subscriber"]`; reserve query/reply for rpc |
| build-ci-4 | dependabot omits the npm ecosystem | `.github/dependabot.yml:3-19` | The published `@sepehrmn/ncp` TS toolchain never gets automated updates *(unverified)* | Add an npm update block |
| build-ci-5 | `.gitignore` lacks Python artifact patterns | `.gitignore:1-9` | `scripts/__pycache__/` noise risks accidental commit *(unverified)* | Add `__pycache__/` and `*.py[cod]` |
| build-ci-6 | ncp-core ships 36 generated TS files to crates.io | `ncp-core/Cargo.toml` + `ncp-core/bindings/` | Bloats the published tarball; cosmetic *(unverified)* | Add `exclude = ["bindings"]` |
| docs-spec-2 | Spec §1 versioning contradicts `check_version`/VERSIONING.md | `NEURO_CYBERNETIC_PROTOCOL.md:60-62` | "checks the major only" is false for 0.x (minor fails closed) | Add the pre-1.0 caveat: while major==0 both must match exactly |
| docs-spec-4 | PLASTICITY.md calls a shipped field (`recordables`) a future `record_from` | `PLASTICITY.md:28-29` | Describes an implemented feature as nonexistent, under a wrong name *(unverified)* | Replace with `RecordTarget.recordables`; reconcile with RATIONALE.md |
| docs-eng-4 | PERFORMANCE.md gives two contradictory speedup ranges | `PERFORMANCE.md:132-134` vs `294-296` | Same benchmark, disagreeing bounds; undercuts the GIL conclusion *(unverified)* | Pick one measured range and use it in both places |
| core-wire-5 (info) | `check_version`/`validate` never invoked on any internal receive path | `ncp-core/src/messages.rs:813,906` | Core never auto-gates version/structure; opt-in only *(unverified)* | Document caller responsibility, or wire into the bus/transport deserialize entry |
| codec-bus-7 (info) | `lerp` degenerate-range guard uses absolute `f64::EPSILON` | `ncp-core/src/codec.rs:29-35` | Dimension-unaware threshold; never bites at O(1)..O(200) ranges *(unverified)* | Scale relative to operands if sub-epsilon ranges ever expected |
| resilience-4 (info) | `burst` recovery time scales with gap size — latched vs momentary undocumented | `ncp-core/src/resilience.rs:135-139,180-182` | A brief severe burst keeps the link flagged jammed for hundreds of good frames *(unverified)* | Document momentary semantics + magnitude-dependent clear; optionally cap CUSUM |
| zenoh-6 (info) | Subscribe callback runs blocking work on the Zenoh callback thread | `ncp-zenoh/src/lib.rs:356-374,404-409` | A blocking user callback can stall delivery; foot-gun for ROS 2 hosts *(unverified)* | Document non-blocking requirement; optionally offer a channel-backed variant |
| gateway-7 (info) | Empty client payload forwarded as a bare newline | `ncp-gateway/src/main.rs:75` | Needless TCP round-trip to learn the request was empty *(unverified)* | Reject empty request payload before connecting |
| cpp-4 (info) | `ncp_check_version` conflates NULL and unparseable into `-1` | `ncp-cpp/src/lib.rs:91-98` | Caller can't distinguish the two (documented) *(unverified)* | Optional: reserve distinct negative codes |
| cpp-5 (info) | NULL `session_id` silently produces an empty key segment | `ncp-cpp/src/lib.rs:123` | Likely a caller bug yields `.../session//sensor` instead of NULL *(unverified)* | Return NULL on NULL/invalid session_id, or document the fallback in ncp.h |
| ts-client-6 (info) | WebSocket transport has no reconnect; drop is terminal + undocumented | `ncp-ts/src/ws.ts:51-53,89-92` | A transient drop latches; README gives no guidance *(unverified)* | Document single-shot semantics, or add optional reconnection |
| ts-client-7 (info) | No tests for the TS client/transport | `ncp-ts/` (no test files) | Seed guard, error surfacing (broken — ts-client-1), FIFO, malformed JSON all unverified *(unverified)* | Add a vitest suite with a mock Send/WebSocket; run in CI |
| build-ci-7 (info) | npm homepage points to `#readme` but README is non-root | `package.json:8,15` | npm page shows the broad protocol README, burying the TS quickstart *(unverified)* | Move TS package to its own dir with package.json beside its README, or accept the root README |
| docs-spec-5 (info) | Spec §3 lists `spike_times`/`weight_set` without the §7 backend caveat | `NEURO_CYBERNETIC_PROTOCOL.md:118,297-298` | Reader may assume backend support is present *(unverified)* | Add a one-line "wire-defined, backend per §7 roadmap" note in §3 |

> Note: findings tagged *(unverified)* carried `status: unverified` in the source data; all others were confirmed under adversarial verification.

---

## 3. Strengths

**Wire-contract discipline (the standout).**
- Enum wire-strings are exact across proto annotations, JSON-Schema `enum` arrays, and Rust serde renames, locked by `lib.rs:64 enum_wire_values` + conformance.rs.
- Rust `#[default]` picks match the JSON-wire defaults (not the proto-3 zero) — a subtle distinction the code gets right.
- `conformance.rs` asserts parity **both directions** (serialized key ∈ schema, schema property ∈ serialized object), plus the `ref_`→`ref` keyword-collision rename and non-default nested-struct cases.
- `required_fields()` matches all 13 schemas' top-level `required` arrays; `validate()` fails closed on non-object JSON, missing/unknown `kind`.
- Forward-compat is real and tested (no `deny_unknown_fields`; an unknown-field payload round-trips).
- The three committed representations (proto, Rust, ts-rs) are genuinely in sync **right now** — both guards pass and the binding trees are content-identical; the guards do catch a dropped annotation.

**Safety governor.** Genuinely fail-closed on the timeout edge cases (the documented `NaN.max(0.0)==0.0` trap is avoided by comparing raw ms); non-finite geometry handled asymmetrically in the safe direction (radius→ESTOP, velocity→HOLD); ESTOP latch dominates and only `reset()` clears it, while `config_fail_closed` deliberately survives reset; HOLD/ESTOP zeroes the union of inbound+negotiated channels preserving arity/unit; the `0=unset` convention reconciled with `Option` via `> 0.0` gating.

**Codec / bus.** NaN/inf sensor samples collapse to the low bound (tested); `lerp` clamps frac to [0,1] so decode output is inherently bounded; `LocalBus` clones handlers before releasing the mutex, avoiding callback-under-lock deadlock; the stale-sensor watchdog only advances on strictly-advancing (t, seq), correctly tripping on a frozen/re-arriving stream.

**Resilience.** `saturating_add` on next-expected bookkeeping (no panic/wraparound at `i64::MAX`); `MAX_GAP_OBSERVE=256` bounds the CUSUM loop while keeping `lost` exact; `loss_rate` guards div-by-zero and stays in [0,1]; ESTOP latching modeled correctly (active() returns None first on latch); `CommandWatchdog` enforces the ttl backstop regardless of producer discipline.

**Zenoh transport.** `check_id()` is the right fail-closed boundary (runtime Err, not just debug-assert) before any key interpolation; subscriber handles deliberately retained against Zenoh's undeclare-on-drop; `request()` distinguishes a remote error reply from a dead server; the module docstring is unusually honest that wire reliability is left at Zenoh's default; `send_command` is correctly fire-and-forget so the sync trait method never blocks the loop.

**Gateway.** Structured `{"kind":"error",...}` frames on every failure path (panic-free, with a hardcoded fallback), matching the cross-language ErrorFrame convention; 30s socket timeouts; `block_in_place` for blocking bridge I/O with a clear rationale; env-overridable config; non-zero exit on startup failure; graceful Ctrl-C shutdown.

**Python / C++ bindings.** Python delegates the wire paths to ncp-core (genuinely byte-identical there) with honest inline docs about the govern no-latch limitation. The C++ FFI is solid: every `extern "C"` body goes through a `catch_unwind` `ffi_guard` (tested, and genuinely load-bearing since no profile sets panic=abort); single-owner CString memory contract with RAII in the demo; strict fail-closed input validation; exact header/export parity today.

**TS client.** Real, correct `Number.isSafeInteger` seed guard matching the proto contract (accepting seed=0 and negatives); the `Wire<T>` bigint→number recursive type mapping correctly distributes over unions and preserves literal enums; WS parse-inside-handler keeps the FIFO queue aligned; strict tsconfig that typechecks cleanly.

**Build / extractability.** Every path dependency stays inside the workspace, so the repo lifts cleanly to standalone; version 0.1.0 / "0.1" is consistent across Cargo, package.json, proto, and the committed dist; the committed `dist/` rebuilds to a zero-byte diff; `gen/` is correctly preview-only and gitignored; CI covers fmt/clippy/build/test, the ts-rs feature build, buf, and the dependency-free Python gates; ncp-python correctly keeps the pyo3 extension-module off by default.

**Docs.** RATIONALE.md is a model of adversarial, honest tech writing (concedes ttl_ms is DDS LIFESPAN, flags the world-writable plane, credits the substrate); VERSIONING.md's pre-1.0 caveat exactly matches `check_version`; the provenance boundary (`calibrated_posterior=false`, `is_simulation_output=true`) is faithfully hard-coded; the README Rust snippet compiles and runs; and the "the governor is not network auth" caveat is stated consistently where it matters.

---

# NCP — Prioritized Action Plan

Synthesized from the 77 verified findings above, aligned to the existing `ROADMAP.md`. Priority key: **P0** = blocks safe/correct pre-1.0 use; **P1** = needed for a credible 1.0; **P2** = important polish; **P3** = nice-to-have.

## P0 — must fix before anyone drives hardware with this

### Theme A — Close the action-plane safety fail-open seams
*Findings: safety-1 (critical), safety-2, safety-3, safety-4, codec-bus-1*
The governor is the safety story, but several paths fail **open** instead of closed/neutral:
- **safety-1 (critical):** `govern()` clamps only tick-0 channels; every predictive `horizon[1..]` setpoint is replayed by `ActionBuffer` **unclamped**, defeating the speed limit + geofence on every tick after the first. This is the headline bug — a UAV can be commanded past its limits through the horizon.
- **safety-2:** staleness check + `CommandWatchdog` fail open on a NaN clock (`NaN > timeout` is false ⇒ "fresh").
- **safety-3:** `config_fail_closed` is skipped when `sensor_channels` is empty, so an unenforceable geofence reports `safety_ok=true`.
- **codec-bus-1:** a missing/renamed population readout decodes to `-1.5 m/s` (full negative velocity), not neutral.
- **safety-4:** fail-closed-to-HOLD paths report `safety_ok=true`, blinding the supervisor.

**Next steps:** apply the speed+geofence clamp to *every* horizon step (or HOLD/reject any horizon containing an over-limit step); treat `!now_s.is_finite()` as stale; drop the `!is_empty()` guard so an active geofence with no sensors fails closed; map a missing population to a documented neutral; add a `last_unsafe` flag so HOLD-on-purpose ≠ unevaluable. Add governor tests for each (esp. "missing pop never yields max-magnitude actuation").

### Theme B — Make the per-plane ACL actually work
*Findings: security-1 (critical), security-3, security-2, security-4*
`SECURITY.md` points operators to `deploy/zenoh-access-control.json5` as the one concrete mitigation for the disclosed world-writable action plane — but as shipped it **does not function**:
- **security-1 (critical):** uses an invalid Zenoh token `"get"`, so the config won't parse / the ACL silently doesn't load.
- **security-3:** no `plant` subject — applying the ACL denies the plant's command-subscribe and sensor-publish, killing the control loop.
- **security-2:** `cert_common_names` glob `controller.*` is matched by exact equality, so real CNs never match.
- **security-4:** controller is granted write on the perception (`sensor/**`) plane — a compromised controller can forge sensor frames.

**Next steps:** fix the tokens (`query`/`declare_subscriber`), add an explicit `plant` subject and make all four roles explicit, fix CN matching, drop the controller's sensor-write grant. **Add a CI check that the json5 deserializes against `zenoh::Config`** so this can never regress silently.

## P1 — needed for a credible 1.0

### Theme C — Make the parity guards cover wire-breaking drift
*Findings: parity-1, parity-2, parity-3, parity-4, parity-5, parity-6, parity-7, parity-9, parity-10*
The README claims "no representation can silently diverge," but the guards are **field-name-set checks only**. They miss exactly what a binary peer cares about: field-type drift (`double↔int64`), field-number re-tagging, enum constant→wire-string inversion, removal of all wire-string annotations (downgrades to a non-fatal note — fail-open), proto-only messages (e.g. `SetpointStep`), and nullable fields (short-circuited). `mode` is also a plain `string` in proto on a safety-critical authority field (parity-6).

**Next steps:** capture `(name, type, number)` and cross-check types + field numbers against a lockfile; make a fully-unannotated wire enum a hard failure; verify per-value enum mapping (not just membership); evaluate `anyOf`/nullable branches; add a reverse pass (every wire proto message must have a schema); model `mode` as the `Mode` enum in proto; expand the conformance corpus to all 13 kinds + safety-critical enum strings (estop!) — this maps directly onto the existing ROADMAP "conformance corpus" item. Scope the README claim to what the guards actually prove.

### Theme D — Fix the published TS package and verify it in CI
*Findings: build-ci-1 (high), build-ci-2, ts-client-1 (high), ts-client-5*  (active-branch focus)
- **build-ci-1:** `moduleResolution:"Bundler"` emits extensionless ESM imports, so `import '@sepehrmn/ncp'` throws `ERR_MODULE_NOT_FOUND` under native Node ESM — the exact documented usage is broken (bundlers masked it).
- **ts-client-1:** the client throws only on a `{kind:'error'}` frame the contract never defines; a real failure (`session_opened{ok:false}`) is silently treated as success — callers proceed against an unopened session.

**Next steps:** switch to `module/moduleResolution: NodeNext` and emit `.js` extensions; after `unwrap`, throw on `reply.ok===false` (and validate `SessionClosed.ok`); finiteness-guard `duration_ms`/`advance_ms` like seed; add a CI smoke test (`npm pack` → install → `node import()`) and a `npm run regen` + `git diff --exit-code` check so `dist/`/`src/generated` drift can't merge.

### Theme E — Bring the Python peer up to its wire-identity claim and test it
*Findings: python-1 (high), python-2, python-4, python-5, python-7*
Wire-identity is a headline claim, but Python's `validate()` does a typed serde round-trip (which the core's own docs call "not honest") instead of calling `ncp_core::validate`, so it accepts frames the Rust peer rejects; `govern()` silently disables the ESTOP latch; and there are **no tests and no working CI path**, so none of this is caught.

**Next steps:** route `validate()` through `ncp_core::validate` on a `serde_json::Value`; expose a stateful governor that latches ESTOP; add the missing glob key-builders; add a maturin+pytest CI job that round-trips the conformance vectors against the Rust peer.

## P2 — important polish

- **Theme F — C ABI header parity** (cpp-2, build-ci-3, cpp-3): the header is hand-written despite docs claiming cbindgen; signature drift = UB at C call sites. Generate via cbindgen + CI diff, or compile-and-link a C TU in CI; fix the docs meanwhile.
- **Theme G — Zenoh QoS truth** (zenoh-1, zenoh-2, zenoh-5, zenoh-3, zenoh-4): docs advertise conflate-to-latest / reliable / KEEP_LAST(1) the code doesn't set. Either implement via the advanced API or align the spec to the actual Drop/Block semantics; close the glob-subscribe `check_id` bypass; add serve_rpc / error / fan-out integration tests.
- **Theme H — Spec & docs accuracy** (docs-spec-1/2/3/4, docs-eng-1/2/4): spec §3 omits normative wire fields + two enum values; §1 versioning contradicts the code; broken `ncp/`-prefixed links to nonexistent files; `bench_realtime.py` `fire_hz` is ~3× inflated (cumulative ÷ one rep) and two docs repeat the wrong justification; PERFORMANCE.md gives two contradictory speedup ranges.

## P3 — nice-to-have / hardening
Remaining low/info items: codec & TS & gateway test modules (codec-bus-6, ts-client-7, gateway-6), `gen/` staleness note, dependabot npm ecosystem, `.gitignore` Python artifacts, `ncp-core` excluding `bindings/` from the crates.io tarball, debug-assert-only key validation (core-wire-3), and the various documentation nits.

## Roadmap alignment
- **Already in `ROADMAP.md`:** action-plane auth/identity (P0) — **Theme B is the interim mitigation for it, and it's currently non-functional**, so it should jump in priority. The "conformance corpus" item maps to **Theme C** (parity-10).
- **Newly surfaced / arguably mis-prioritized:** **Theme A** (safety fail-open seams) and **Theme C** (parity-guard gaps) are largely *new* and undercut guarantees the repo already advertises as done ("safety-gated", "no representation can silently diverge"). Both deserve to sit at/above existing roadmap work. **Theme D** (TS ESM breakage) blocks the very package this branch ships.

## Top risks (ranked)
1. A robot/UAV is commanded past its speed/geofence limits because the predictive horizon bypasses the clamp (**safety-1**).
2. Operators deploy believing the ACL protects the action plane, but it never loads (**security-1**) — and even fixed, it would deny the plant (**security-3**).
3. A binary-wire peer silently diverges (type/field-number/enum-mapping) because the parity guards don't check for it, despite the README's claim (**Theme C**).
4. The `@sepehrmn/ncp` package is unusable as published under Node ESM (**build-ci-1**), and a failed session-open is silently treated as success (**ts-client-1**).
5. The Python peer's wire-identity is unverified and demonstrably weaker (validate + ESTOP) than the Rust reference (**Theme E**).

---
*Generated by a 14-dimension multi-agent review + adversarial verification (69 agents, 90 findings → 77 confirmed / 13 refuted). The plan was synthesized by the main loop after the plan-synthesis agent died on a transport error; all underlying findings are from the verified workflow output.*
