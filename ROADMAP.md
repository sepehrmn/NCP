# NCP Roadmap

> A prioritized, honest improvement plan for the Neuro-Cybernetic Protocol (NCP)
> SDK and wire contract. It distills a deep-research review of NCP against the
> 2025–2026 agent-protocol, robotics-middleware, RL/simulation-RPC, and
> neuroscience-co-simulation landscape into pre-1.0 work. Read `RATIONALE.md` for
> why NCP exists and what it deliberately borrows; read `SECURITY.md` for the
> current threat model and its disclosed limitation. This roadmap does not repeat
> those documents — it sequences the work.

## Status: what NCP v0.x is, and is not

NCP v0.x is a versioned, transport-agnostic wire contract that serves a running
NEST point- and rate-neuron network to external robot/UAV/analysis clients over
QoS-differentiated Zenoh planes, with scientific provenance baked into every frame
(`is_simulation_output=true`, `calibrated_posterior=false`) and a safety-gated
action plane (`mode ∈ {init,active,hold,estop}`, `ttl_ms` fail-safe). It is a
**control artifact, not a validated scientific reproduction** — output is never a
paper-reproduction claim, and the provenance discriminators are mandatory and
fail-closed precisely to keep that boundary machine-checkable. It is a single
reference SDK (proto-native — `proto/ncp.proto` normative, `ncp-core` the Rust
reference implementation; Python via PyO3, TypeScript types via ts-rs, a C ABI for
C/C++) with field-set-parity drift guards, not yet a multi-implementation program. It is **pre-1.0** (current wire `0.2`, released as `v0.2.0`/`v0.2.1`/`v0.2.2`/`v0.2.3`/`v0.2.4`): the wire
may change, minor versions are treated as breaking, and the version guard fails
rather than silently coercing. NCP's contribution is a typed, provenance-first, safety-gated wire
contract — not novel control science and not the first SNN-in-the-loop robot loop
(that lineage belongs to MUSIC and the ROS-MUSIC toolchain; see "Honest positioning").

---

## P0 — Authenticate the action/command plane

**This is the #1 gap, honestly disclosed in `SECURITY.md`.** Today the
action/command plane is unauthenticated and effectively world-writable on an open
realm: any participant that can reach the realm can publish actions, and
`controller_id` is self-asserted. The local `mode`/`ttl_ms` governor is
defense-in-depth, not network security.

**Landed (#7):** a default-deny, per-plane Zenoh ACL template
([`deploy/zenoh-access-control.json5`](deploy/zenoh-access-control.json5)) in which
only an authenticated commander may publish commands and observers are read-only,
plus concrete mutual-TLS + ACL enablement steps and the DDS-Security / MAVLink-2
comparators in [`SECURITY.md`](SECURITY.md).

**Landed (hardening pass):** the ACL template is now **loadable** — it previously
used `"get"` in `messages`, which is not a valid Zenoh token, so zenohd would
reject the whole config (the mitigation read as "secured" while doing nothing).
Fixed to the correct tokens (`query` / `declare_subscriber`), clarified the
exact-string `cert_common_names` matching, and added `scripts/check_acl_template.py`
(CI guard: invalid-token detection + the "only `engram` may PUT on `…/command/**`"
safety invariant), wired into `scripts/check.sh`.

- **Validate the mTLS-bound identity in a live deployment.** The remaining P0 work
  is exercising the ACL + mutual-TLS enforcement end-to-end (a perception-only
  identity is *rejected* on the action plane; only the commander succeeds), so the
  `controller_id` is *proven* by the transport rather than self-asserted. *Why:*
  this closes the textbook confused-deputy / world-writable failure class — the
  template now ships a *loadable* mechanism; only live enforcement validation
  remains (it needs a real deployment, so it is out of CI's reach).

P0 is the gate for any deployment beyond a trusted, closed realm. Until live mTLS
enforcement is validated, the `SECURITY.md` "closed realm only" guidance stands.

---

## P1 — Identity & capability negotiation

- **Replace the ad-hoc self-asserted id with a standards-grade identity.** Adopt
  W3C Decentralized Identifiers (DIDs, e.g. `did:wba`) plus verifiable credentials,
  or an explicit capability handshake, as the open-realm identity model; for a
  closed-realm v0, mTLS client certs / Zenoh auth are the pragmatic mechanism with
  DID as the open-realm path. *Why:* DID + verifiable-credential identity is the
  established alternative to reinventing an identity scheme, and reviewers measure a
  protocol on authentication-mode diversity, not on a bespoke id field.
- **Negotiate capabilities at `open_session`.** Have peers advertise and verify
  which planes they may use (perception, action, both, neither) as part of the
  control-RPC handshake, rather than implicitly trusting a connecting peer. *Why:*
  capability negotiation makes the per-plane ACL from P0 a first-class, auditable
  contract instead of an out-of-band convention.

---

## P1 — Versioning: from local guard to negotiated, pinned handshake

NCP's current `check_version` correctly fails closed (it compares the received
`ncp_version` against the locally compiled version and refuses a mismatch rather
than coercing). But it is a one-sided local guard with no integrity binding.

- **Make version a peer-negotiated handshake on the control plane.** Surface a
  typed incompatibility from a two-way exchange, not just a local reject. *Why:*
  this turns "I refuse" into "we agreed (or explicitly did not)," which is what a
  multi-peer protocol needs.
- **Pin and verify the wire-contract definition. (Landed.)** `ncp_core::CONTRACT_HASH`
  (FNV-1a of `proto/ncp.proto`) + `verify_contract` + `negotiate(version, hash)` let
  peers reject a post-agreement schema mutation; a conformance test recomputes the
  hash from the real proto so a forgotten bump fails CI. *Remaining:* carry the hash
  in the control-plane handshake envelope (today `negotiate` takes it as a param),
  and upgrade FNV → a signed/cryptographic digest if the threat model needs
  adversarial (not just accidental) integrity.
- **Keep failing closed. (Hardened.)** `check_version` no longer coerces a malformed
  minor to 0 (the latent fail-open the review found): minor parsing is now as strict
  as major. *Remaining:* the documented "minor is breaking" rule + a
  backward-compatibility (upgrade-success) check in the conformance suite.

---

## P1 — Conformance: from parity test to a shared golden corpus

`conformance.rs` checks field-set and required-array parity between serialized Rust
messages and the JSON Schemas; `scripts/check_proto_schema_parity.py` adds the
`proto/ncp.proto` ↔ JSON Schema side (field-set + enum wire-string parity). Real
drift guards, but intentionally not full validation, with no coverage of version
negotiation or the safety governor.

**Landed (#9):** a golden-vector corpus — JSON message vectors *and* a binary
bulk-codec vector — in [`conformance/vectors/`](conformance/vectors/), validated by
`scripts/check_conformance_vectors.py` (with a stdlib reference decoder any peer can
run); a `buf breaking` WIRE/WIRE_JSON gate against the `v0.2.0` baseline; and
[`GOVERNANCE.md`](GOVERNANCE.md) documenting the governance model + the mechanical
interop gates + the neutral-home path.

- **Extend the corpus to behavioral outcomes across every binding.** Add expected
  outcomes for `validate()`, `check_version()`, and the safety-governor
  (`mode`/`ttl_ms`) behavior, and have the Rust, Python, TS, and C peers all run
  against them. *Why:* the message/field corpus proves wire shape; behavioral
  vectors make "`ncp-core` is the reference implementation" a *verifiable* claim for
  every binding, not just the Rust one.
- **Scope it honestly.** Conformance here is implementation-vs-spec compliance, not
  interoperability (which would require multiple independent implementations
  interacting). Do not claim alignment to formal ISO/IEC 9646 / ETSI methodologies;
  a pragmatic golden-fixture corpus is the appropriate bar.

---

## P2 — Observability

- **First-class OpenTelemetry spans** across the control RPC and the data planes,
  and an **append-only run-log as the source of truth** for what was commanded,
  observed, and rejected. *Why:* the agent-protocol literature repeatedly flags
  missing/optional observability and missing result provenance as a maintainability
  failure mode; an append-only log also gives the safety governor's reject
  decisions an auditable trail.

---

## P2 — Packaging & citation

- **Dual MIT OR Apache-2.0 licensing. (Landed.)** Moved from MIT-only to the
  `MIT OR Apache-2.0` crates.io convention: `LICENSE` → `LICENSE-MIT`, added
  `LICENSE-APACHE`, and updated `Cargo.toml`, `package.json`, `CITATION.cff`, and
  the README. *Why:* it is the Rust-ecosystem norm and removes a friction point for
  downstream adoption.
- **PyPI wheels via maturin.** Build the PyO3 binding into wheels (consider `abi3`
  so one wheel covers CPython 3.8+ per platform) and add a `pyproject.toml`. *Why:*
  maturin is the canonical PyO3-to-PyPI path; a published wheel is table stakes for
  the Python peer to be usable without a Rust toolchain.
- **Zenodo DOI via the GitHub–Zenodo archive.** Tagged releases now exist
  (`v0.2.0`/`v0.2.1`/`v0.2.2`/`v0.2.3`/`v0.2.4`); enable the GitHub–Zenodo integration so a release is archived
  and a DOI minted, then add it to the existing `CITATION.cff`. *Why:* a DOI is the
  minimum citable artifact; the repo currently has a `CITATION.cff` with no DOI.
- **Defer JOSS.** A JOSS submission is viable only after roughly six months of
  public history with genuine ongoing iteration, a substantial (not thin) SDK, and
  demonstrated research impact. Until then, prefer arXiv plus a robotics /
  neuromorphic workshop for the protocol write-up. *Why:* JOSS desk-rejects
  short-history "single burst of commits" repos and thin API wrappers; planning
  around the timing gate is more honest than rushing it.

---

## P3 — Performance (measure first)

- **Reduce per-frame allocation and clone churn on the hot path.** Hold a persistent
  Zenoh `Publisher`, precompute key expressions once, and avoid per-frame `String`
  / `Vec` allocation in the perception/action loops. *Why:* the data planes are the
  perpetual sub-10 ms path, and steady-state allocation is the obvious cost — but
  this is P3 deliberately: **measure before optimizing**, and do not claim latency
  leadership (a software NEST-over-Zenoh loop will not approach on-chip neuromorphic
  figures, which are task-specific demos).

---

## P2 — Real-time honesty & sizing (measured)

A benchmark sweep on NEST 3.8.0 (16 cores) put numbers on §7 of
[`NEST_REALTIME.md`](NEST_REALTIME.md): for a Brunel-style net at ~500
syn/neuron and ~13 Hz, **>=1x real time is reached only at N=10000 and only at
>=4 threads** (T=8 = 2.01x); no N>=50000 config reaches real time on 16 cores
(best N=50000 T=16 = 0.35x). A live NCP session that exceeds that budget will
silently lag wall-clock. These items make the lag honest and give the principled
mitigations. Full numbers + method in [`NEST_REALTIME.md`](NEST_REALTIME.md) and
[`PERFORMANCE.md`](PERFORMANCE.md); reproduce with `scripts/bench_realtime.py` and
`scripts/bench_overlap.py`.

- **`open_session` real-time-budget check + telemetry.** At session open, given the
  network size, `sim.chunk_ms`, and the requested control rate, estimate the
  real-time factor (or measure it from a short untimed warmup, as the benchmark
  does) and **warn / refuse-as-non-realtime** when the loop cannot keep up — then
  surface the achieved real-time factor and per-chunk wall time in `ControlStatus`
  telemetry. *Why:* the sweep shows the binding constraint is the real-time factor
  (compute vs wall), **not** the chunk size — shrinking `chunk_ms` buys latency, not
  throughput, and while compute-bound makes it worse (per-`Run()` overhead climbs).
  A session should **fail honest** (declared offline / sub-real-time) rather than
  advertise a live loop it cannot sustain. This also fits the provenance-first
  posture: a real-time claim becomes a checked discriminator, not an assumption.

- **Run transport on a native thread, off the NEST *Python* thread (GIL-grounded,
  measured).** `nest.Run()` holds the Python GIL for essentially its full duration,
  so an in-process *Python* thread overlaps transport with compute only ~1.0–1.25×.
  A **native OS thread**, however, overlaps fully — measured **1.68×** for a C
  pthread (a faithful proxy for a Rust `std::thread` / PyO3 background thread) vs
  **1.08×** for a Python thread ([`scripts/bench_gil_overlap.py`](scripts/bench_gil_overlap.py)).
  So run the per-tick transport (CommandFrame/SensorFrame serialization + Zenoh RTT)
  on a native thread: either the **Rust gateway / a separate process** (recommended —
  also isolates the loop from Python GC jitter) or an **in-process PyO3 background
  thread**. (Releasing the GIL inside PyNEST via Cython `with nogil` would also work,
  but requires an upstream NEST patch.) *Why:* this is the configuration in which
  transport I/O actually overlaps compute; it codifies that the NEST kernel and the
  transport stack must not share the *Python* thread — not merely the interpreter.

- **`CommandFrame.horizon` + `ttl_ms` HOLD as the principled real-time mitigation.**
  When the real-time budget cannot be met (or the link drops), the actuator replays
  the predictive `horizon` setpoints — each entry expiring at its own
  `t + i·horizon_dt_ms`, capped by `ttl_ms`, then **HOLD fires** (the `ActionBuffer`
  / `CommandWatchdog` in `ncp-core::safety`; see [`RESILIENCE.md`](RESILIENCE.md)).
  *Why:* this is exactly the lever for the sub-real-time / lagging-loop regime the
  sweep exposes — predictive lookahead buys `N · horizon_dt_ms` of ride-through, and
  a bounded HOLD is the honest fail-safe when the brain cannot keep pace, rather than
  pretending a stale command is current.

- **Distributed / MPI-NEST as the path to bigger live brains.** The sweep was
  OpenMP-only, single MPI rank; memory was never the limiter (~5 GB at 100M
  synapses) — **compute/wall time was**, and it degrades ~linearly with synapse
  count. *Why:* the only way to push the ~10k–20k-neuron live ceiling materially
  higher is MPI scale-out across ranks/nodes (or lower indegree). NCP serving a
  multi-rank NEST kernel is the natural growth path; document it as the route to
  larger real-time brains rather than implying single-node scales indefinitely.

- **Sensible default `local_num_threads`.** Default the NEST backend toward the
  **4–8 thread band**, not 1 and not blindly 16. *Why:* efficiency peaked there
  (super-linear, cache-driven — N=50000 T=8 efficiency ~1.12) and collapsed to ~0.66
  at T=16; 1 thread leaves >6x on the table and all-16 wastes ~30–35% to
  memory-bandwidth/synchronization contention. A measured default beats an arbitrary
  one. (Expose it; do not hard-code — the right value is hardware-dependent.)

---

## Future direction: simulator-agnosticism (a second backend)

NCP's wire is **simulator-agnostic by design, NEST-implemented in fact.** The
typed record/stimulus vocabulary (`V_m`, `spikes`, `rate`, `weight`, `current_pA`,
`rate_hz`, `spike_times`) are abstract spiking-network concepts, **not** NEST APIs;
each `SimulationBackend` maps them to its simulator (NEST `V_m` ↔ NEURON `v`,
`current_pA` ↔ NEURON `IClamp`, `spikes` ↔ NEURON `NetCon`). The simulator-specific
long tail — model recordables (`g_ex`, `S`), connection params (siegert
`drift_factor`/`diffusion_factor`) — rides the generic `recordables[]` / `params{}`
escape hatches (#10), which the backend resolves. The only NEST-shaped leakage is
in free strings the backend owns, not the typed wire.

**This costs the NEST path nothing.** Adding a NEURON / Brian2 / GeNN /
neuromorphic backend is a *new `SimulationBackend` implementation* in the host
simulation service (the seam is the backend abstraction, not the wire), **not a wire
change** — it cannot slow or degrade NEST. NEST is the reference and only implemented backend today, so
simulator-agnosticism is a **designed property, not yet a shipped one**: no second
backend exists. When one lands, the points to abstract are `NetworkRefKind::Builtin`
(a NEST model name) and the recordable-string conventions, and the `VERSIONING.md`
promotion rule applies (a recordable common across simulators graduates to a typed
enum variant, additively).

## #10 neuron-family coverage (landed in v0.2.0)

`#10` shipped in **v0.2.0** (reference backend verified on NEST 3.9), extending the
wire **additively**: `Observable.binary_state`, `StimulusKind.rate_inject`,
`RecordTarget.recordables`, `Observation.recordable`, `StimulusTarget.params` —
covering conductance (`g_ex`/`g_in`/`w`), rate models, and binary state via a
generic named-recordable + named-param mechanism, wired in the reference backend via
NEST `multimeter`/`step_rate_generator`/`spin_detector`. The wire `ncp_version` was
bumped `0.1`→`0.2`, so the version guard fires on the new enum values — an old `0.1`
peer is fail-closed rejected rather than silently dropping a frame carrying
`binary_state`/`rate_inject` (the enums have no `serde(other)` fallback), and
downstream consumers re-pin to `tag=v0.2.0` to speak the new wire.

**Remaining:** niche driving (binary `noise_generator`, siegert
`diffusion_connection`) needs a driver-neuron topology; and stamping observations
with the driving `seq` so a split-plane observer can align its dynamics channel on
`seq` rather than arrival time. Optionally add `#[serde(other)]` enum fallbacks for
graceful cross-minor degradation.

## #6 bulk observation codec (landed in v0.2.0)

The observation/analysis plane can carry bulk numeric arrays (spike trains, `V_m`
traces) as a packed little-endian column block — `ncp-core::bulk` (`BulkBlock`) +
the proto `BulkObservation` envelope — parse-free and ~2× smaller than `repeated
double`, byte-pinned across languages by a binary conformance vector. It is an
**additive, negotiated, observation-plane-only** option; the canonical JSON
`ObservationFrame` stays the always-available representation, and the hot action
loop never uses it. See [`PERFORMANCE.md`](PERFORMANCE.md).

## Honest positioning

NCP's closed-loop spiking-neural / NEST-in-the-loop story is **not** a new control
science result and **not** the first SNN-robot loop. Prior neurorobotics
middleware already did real-time simulation-to-robot closed loops — MUSIC and the
ROS-MUSIC toolchain established the continuous/event channel taxonomy and a
NEST-to-robot loop, and the HBP Neurorobotics Platform generalized the data-model
side. NCP's actual, defensible contribution is a **typed, provenance-first,
safety-gated wire contract with a conformance program** that complements (does not
replace) that neuroscience middleware and ROS 2 / DDS robotics middleware. Any
write-up must frame it that way, keep the provenance discriminators mandatory and
conformance-checked, and explicitly disclaim that NCP output is a control artifact,
never a validated neuroscientific reproduction.

---

## Non-goals (for now)

- **Multi-commander / federation.** Coordinating multiple simultaneous commanders,
  cross-realm federation, and multi-writer "who-steps-when" arbitration are deferred
  past 1.0; v0.x assumes a single controlling authority per realm.
- **Not a substitute for network security.** Even with the P0 work landed, NCP's
  safety governor and ACLs are defense-in-depth on top of a properly secured
  realm — they are not, and will not claim to be, a replacement for network-level
  authentication and isolation.
