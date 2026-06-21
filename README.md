# NCP — Neuro-Cybernetic Protocol

> A single versioned, typed, cross-language **wire contract** for a running NEST point- and rate-neuron simulation (spiking, binary, and rate-based models) to perceive and act through robots, UAVs, and analysis clients — safety-gated and provenance-first.

[![CI](https://github.com/sepahead/NCP/actions/workflows/ci.yml/badge.svg)](https://github.com/sepahead/NCP/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://www.rust-lang.org)
[![status: experimental](https://img.shields.io/badge/status-experimental%20(pre--1.0)-orange.svg)](#status)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)
[![Code of Conduct](https://img.shields.io/badge/Contributor%20Covenant-2.1-purple.svg)](CODE_OF_CONDUCT.md)
[![Cite](https://img.shields.io/badge/cite-CITATION.cff-blueviolet.svg)](CITATION.cff)

## What is NCP

NCP is a versioned, transport-agnostic wire contract that lets a running NEST network — point neurons (spiking, binary) and rate-based models — serve external robot, UAV, and analysis clients — for **perception, action, both, or neither** — over QoS-differentiated planes with a safety-gated action plane and scientific provenance on every frame. The reference implementation is a Rust SDK with Python, C/C++, and TypeScript peers that all speak the identical wire.

**Honesty boundary (binding):** returned `V_m`/spikes are raw simulation outputs of a specified model, never a validated reproduction. Every frame carries `is_simulation_output=true` and `calibrated_posterior=false`. A neuro-controller is a **control artifact**, never a paper-reproduction claim.

## Topology

One commander (an Engram/NEST brain) coordinates one or more bodies over four QoS planes, each carrying the reliability, priority, and conflation its job needs.

```mermaid
flowchart LR
    subgraph Commander["Commander — Engram / NEST brain"]
        BRAIN["neural network · point + rate<br/>(perception · action · both · neither)"]
    end

    subgraph Bodies["Bodies & clients"]
        ROBOT["robot / UAV body"]
        OBS["analysis / observer client"]
    end

    BRAIN -- "/rpc · control · reliable request/reply" --> ROBOT
    ROBOT -- "/sensor · perception · best-effort, conflating" --> BRAIN
    BRAIN -- "/command · action · express · RealTime · safety-gated (mode, ttl_ms)" --> ROBOT
    BRAIN -. "/observation · read-only tap" .-> OBS
    ROBOT -. "/observation · read-only tap" .-> OBS
```

| Plane | Key | QoS | Purpose |
|---|---|---|---|
| **Control** | `{realm}/rpc` | reliable, request/reply (queryable) | session lifecycle |
| **Perception** | `{realm}/session/{id}/sensor[/{name}]` | best-effort DROP (lossy-OK) | plant → brain |
| **Action** | `{realm}/session/{id}/command[/{name}]` | express, RealTime, safety-gated (`mode`, `ttl_ms`) | brain → plant |
| **Observation** | `{realm}/session/{id}/observation` | read-only pub/sub | free diagnostic tap |

Because the data planes are pub/sub, **observers attach for free**: an analysis client subscribes read-only with zero changes to the control path — the structural reason to choose a data-centric bus over point-to-point RPC for a fleet plus watchers.

## Highlights

- **Transport-agnostic core.** `ncp-core` is `serde`-only — no transport, no async. The wire is JSON/protobuf, identical across mediums; Zenoh is the recommended (and currently shipped) transport.
- **Four QoS planes.** Control RPC, conflating perception, express RealTime action, and a read-only observation tap — each pays only the cost its job needs.
- **Safety-gated action plane.** A `mode` enum (`init`/`active`/`hold`/`estop`) is an explicit wire authority, backed by a latched ESTOP, `ttl_ms` HOLD fail-safe, a fail-closed command watchdog, and geofence checks.
- **Per-frame provenance.** `is_simulation_output` and `calibrated_posterior` are mandatory, fail-closed fields — a machine-checkable epistemic discriminator on the hot path.
- **Neuron-family coverage.** A generic named-recordable + named-parameter wire (`recordables[]`, `recordable`, `params{}`, plus the `binary_state` observable and `rate_inject` stimulus) serves NEST's point, conductance (`g_ex`/`g_in`/`w`), binary, and rate-based families — not just spiking.
- **Bulk observation codec.** For large spike trains / `V_m` traces, the observation plane can carry a packed little-endian column block (`ncp-core::bulk`, proto `BulkObservation`): parse-free, random-access, ~2× smaller than `repeated double` — additive and observation-plane only (never the hot action loop).
- **Conformance-tested wire (proto-native).** `proto/ncp.proto` is the normative contract; parity guards + a golden-vector corpus keep everything in lock-step — `conformance.rs` (Rust serde ↔ JSON Schema), `check_proto_schema_parity.py` (proto ↔ JSON Schema), JSON **and** binary golden vectors (`conformance/vectors/`), and a `buf breaking` WIRE/WIRE_JSON gate — so no representation can silently diverge.
- **Authenticatable action plane.** A default-deny, per-plane Zenoh ACL template (`deploy/zenoh-access-control.json5`) + mutual-TLS enablement steps let only an authenticated commander publish commands; observers stay read-only. The open-realm default is unauthenticated until this is enabled (see `SECURITY.md`).
- **Polyglot peers.** `proto/ncp.proto` is normative; `ncp-core` is the reference implementation. Python via PyO3, a C ABI for C/C++, and TypeScript types via ts-rs — every peer is wire-identical off the same contract, so the safety/codec logic is written once, not reimplemented per language.

## Crates

| Crate | Role |
|---|---|
| **`ncp-core`** | Pure protocol: wire types (serde), version guard, key scheme, reference rate codec, action-plane safety governor, in-process bus + control loop. `serde`-only, no transport. |
| **`ncp-zenoh`** | The recommended decoupled transport: Zenoh *queryable* (control RPC) + *pub/sub* (perception/action/observation), each with its plane's QoS. |
| **`ncp-python`** | Python binding (PyO3): the Rust core as an importable `ncp` module, so Python peers are wire-identical without reimplementing. |
| **`ncp-cpp`** | C / C++ binding: a stable C ABI (`extern "C"` + `include/ncp.h`) over the same core. |
| **`ncp-ts`** (`@sepehrmn/ncp`) | TypeScript package: wire types generated from `ncp-core` (ts-rs) + a transport-agnostic client and a WebSocket transport — wire-identical to the Rust/Python peers. |
| **`ncp-gateway`** | The commander's Rust edge (reference deployment, e.g. an Engram/NEST host): runs the Zenoh bus and bridges control-plane RPC to a simulation `SessionService` over a localhost socket (NEST stays Python). |

## Polyglot quick-start

One normative wire ([`proto/ncp.proto`](proto/ncp.proto) / [`NEURO_CYBERNETIC_PROTOCOL.md`](NEURO_CYBERNETIC_PROTOCOL.md)); pick the peer for your language. Each per-peer README below is the deep doc — this matrix is the index.

| Peer | Install / depend | Open session · step · observe | Transport(s) |
|---|---|---|---|
| **`ncp-core`** (Rust) | `ncp-core = { git = "https://github.com/sepahead/NCP", tag = "v0.4.0" }` | Build `OpenSession` / `CommandFrame`, `serde_json::to_string` → wire — see [`ncp-core/README.md`](ncp-core/README.md) | none (serde-only; in-process bus + control loop) |
| **`ncp-zenoh`** (Rust transport) | `ncp-zenoh = { git = "https://github.com/sepahead/NCP", tag = "v0.4.0" }` | `let bus = ZenohBus::open().await?; let client = ZenohNcpClient::new(bus); client.open(&msg).await?` — see [`ncp-zenoh/README.md`](ncp-zenoh/README.md) | Zenoh (queryable RPC + per-plane pub/sub) |
| **`ncp-python`** (Python / PyO3) | `maturin develop -m ncp-python/Cargo.toml --features extension-module` | `import ncp; ncp.Keys("engram/ncp").command("uav3"); ncp.decode_command(...)` — see [`ncp-python/README.md`](ncp-python/README.md) | transport-agnostic (JSON wire via `ncp-core`) |
| **`ncp-cpp`** (C / C++ ABI) | `cargo build -p ncp-cpp` → link `libncp_cpp`, `#include "ncp.h"` | `char *v = ncp_version(); /* ... */ ncp_string_free(v);` — see [`ncp-cpp/README.md`](ncp-cpp/README.md) | transport-agnostic (JSON in/out over the C ABI) |
| **`ncp-ts`** (`@sepehrmn/ncp`, TypeScript) | `npm install @sepehrmn/ncp` | `const ncp = new NeuroSimClient(transport.send); await ncp.open(...); await ncp.step(...); await ncp.close(...)` — see [`ncp-ts/README.md`](ncp-ts/README.md) | WebSocket (`WebSocketNeuroSim`) or any `Send` bus |

## Quickstart

NCP is **not yet published to crates.io** (pre-1.0). Depend on it as a pinned git dependency:

```toml
[dependencies]
ncp-core  = { git = "https://github.com/sepahead/NCP", tag = "v0.4.0" }
ncp-zenoh = { git = "https://github.com/sepahead/NCP", tag = "v0.4.0" }  # transport, optional
```

A minimal, wire-correct snippet using `ncp-core` — build a safety-gated `CommandFrame`, then refuse an incompatible peer version:

```rust
use ncp_core::{check_version, ChannelValue, CommandFrame, Mode, NCP_VERSION};

// A controller's actuation, gated by mode + a time-to-live fail-safe.
let cmd = CommandFrame {
    seq: 42,                       // echoes the SensorFrame.seq it was computed from
    mode: Mode::Active,            // init / active / hold / estop
    ttl_ms: 200.0,                 // HOLD fires if the actuator outlives this
    channels: [(
        "velocity_setpoint".to_string(),
        ChannelValue::vec3(0.5, 0.0, -0.2, Some("m/s")),
    )].into_iter().collect(),
    ..Default::default()
};
let wire = serde_json::to_string(&cmd)?;   // wire-identical to the Python / TS peers

// Fail closed on an incompatible peer (pre-1.0: minor is breaking).
assert!(check_version(NCP_VERSION, true)?);     // exact match -> Ok(true)
assert!(check_version("0.9", true).is_err());   // 0.x minor diff -> rejected
```

- **Spec:** [`proto/ncp.proto`](proto/ncp.proto) is the normative wire contract (proto-native — language bindings generate from it via buf). The JSON Schemas in [`schemas/`](schemas/) are its JSON projection, kept in lockstep with the proto by the parity guard (`scripts/check_proto_schema_parity.py`); today they are emitted from the reference Pydantic models (see [`schemas/README.md`](schemas/README.md)), with proto-native schema generation a tracked decoupling item. [`NEURO_CYBERNETIC_PROTOCOL.md`](NEURO_CYBERNETIC_PROTOCOL.md) is the human-readable spec.
- **Conformance + benchmarks:**

```bash
scripts/check.sh              # full conformance / smoke matrix (all crates + bindings)
cargo test -p ncp-core        # pure, fast: wire-compat + codec + safety + control loop
python scripts/bench_realtime.py   # NEST real-time-factor sweep (see NEST_REALTIME.md)
python scripts/bench_overlap.py    # transport/compute overlap (GIL) measurement
```

## Spec & documentation

- [`NEURO_CYBERNETIC_PROTOCOL.md`](NEURO_CYBERNETIC_PROTOCOL.md) — the protocol spec (messages, planes, entity model).
- [`RATIONALE.md`](RATIONALE.md) — why NCP exists, adversarially reviewed against ROS 2/DDS, Zenoh, MUSIC, the Neurorobotics Platform, MCP/ACP, gRPC, and dm_env_rpc.
- [`RESILIENCE.md`](RESILIENCE.md) — robustness over a poor/jammed link: predictive replay, fail-safe HOLD, watchdog, link monitor.
- [`PERFORMANCE.md`](PERFORMANCE.md) — does NCP bottleneck NEST? The one real bottleneck found and fixed, plus a per-tick cost model.
- [`NEST_REALTIME.md`](NEST_REALTIME.md) — can NCP read NEST live without stopping it (like MUSIC)? Yes; measured real-time-factor sweep.
- [`ROADMAP.md`](ROADMAP.md) — the prioritized, honest pre-1.0 plan (auth, identity, conformance corpus, observability).
- [`VERSIONING.md`](VERSIONING.md) — the SemVer wire policy, the `buf breaking` enforcement, and the pin guidance.
- [`GOVERNANCE.md`](GOVERNANCE.md) — the governance model, the mechanical interop gates, and the path to a neutral home.
- [`SECURITY.md`](SECURITY.md) — threat model, the disclosed action-plane limitation, and the TLS + ACL enablement steps.
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — how to build, test, and propose changes.
- [`CHANGELOG.md`](CHANGELOG.md) — per-release notes (current: `v0.4.0`).

## Status

NCP is **pre-1.0 and experimental.** Specifically:

- **The wire may change.** Minor versions are treated as breaking; the version guard fails closed rather than coercing. **Pin the latest tag** (`tag = "v0.4.0"` above — the wire is `0.4`, with `v0.4.0` the buf-breaking baseline) for anything you build against.
- **Single reference implementation.** `proto/ncp.proto` is the normative contract; `ncp-core` (Rust) is the reference implementation and Python/C/TS are bindings off the same contract, verified by field-set-parity drift guards — not yet a multi-implementation conformance program.
- **The action plane is currently unauthenticated.** On an open realm it is effectively world-writable: anyone who can reach the realm can publish commands. The local `mode`/`ttl_ms` governor is defense-in-depth, **not** network security. Deploy only on a trusted, closed realm. See [`SECURITY.md`](SECURITY.md) and the P0 work in [`ROADMAP.md`](ROADMAP.md).

## Citing

A Zenodo DOI will be minted when the project is archived to Zenodo; until then, cite the repository (see [`CITATION.cff`](CITATION.cff)).

```bibtex
@software{mahmoudian_ncp,
  author  = {Sepehr Mahmoudian},
  title   = {NCP — Neuro-Cybernetic Protocol},
  year    = {2026},
  version = {0.4.0},
  url     = {https://github.com/sepahead/NCP}
}
```

## Contributing

Contributions are welcome. Please read [`CONTRIBUTING.md`](CONTRIBUTING.md) for the build/test workflow and [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) (Contributor Covenant 2.1) for community expectations.

## License

Dual-licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at
your option (the Rust-ecosystem convention). © 2026 Sepehr Mahmoudian.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual-licensed as above, without any additional terms or conditions.

## Acknowledgements / prior art

NCP builds in the spirit of established work and claims no novel control science:

- **MCP / ACP** — versioned, schema-first capability handshakes for agents inform NCP's control-plane versioning and capability negotiation.
- **MUSIC** (Djurfeldt et al. 2010) and the **ROS-MUSIC toolchain** (Weidel et al. 2016) — the continuous-(V_m/rate) vs event-(spikes) channel taxonomy and the first real-time NEST-to-robot closed loops; NCP is informed by, not a replacement for, this lineage.
- **HBP Neurorobotics Platform / NRP-core** — the closest data-model prior art for declaring what to record and inject.
- **Zenoh** — the data-centric transport whose queryables, conflation, express priority, and routed subscriptions NCP inherits (credited to the substrate, not invented here).

NCP's actual contribution is a **typed, provenance-first, safety-gated wire contract** that complements this work — not novel control science, and not the first SNN-in-the-loop robot loop.
</content>
</invoke>
