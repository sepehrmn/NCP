# NCP — Neuro-Control Protocol (Rust reference SDK)

A versioned, **transport-agnostic, project-agnostic** standard for letting an
Engram-driven NEST simulation serve external robot / UAV / simulation systems —
for **perception, action, both, or neither**. This workspace is the **normative
Rust reference implementation**; NCP is intended to become a reusable standard
(in the spirit of MCP/ACP), and Rust is the canonical high-performance
implementation.

Why NCP exists at all — an unbiased rationale vs ROS 2/DDS, Zenoh, MUSIC, the
Neurorobotics Platform, MCP/ACP, gRPC, dm_env_rpc, and the "compose, don't invent"
alternative — is in [`RATIONALE.md`](RATIONALE.md).

The human-readable spec is [`../NEURO_CONTROL_PROTOCOL.md`](../NEURO_CONTROL_PROTOCOL.md);
the polyglot payload contract is [`../backend/neurocontrol/ncp.proto`](../backend/neurocontrol/ncp.proto)
with JSON-Schema mirrors in `../backend/neurocontrol/schemas/`. The Rust types
here serialize to **exactly** that JSON, so the Rust, Python and TypeScript peers
interoperate over any transport.

> **Extractable by design.** This workspace has no dependency on the surrounding
> Paper2Brain repo and can be lifted into its own repository / published to
> crates.io with no refactoring. It lives here because Paper2Brain is the
> protocol's documented home.

## Crates

| Crate | Role |
|---|---|
| **`ncp-core`** | Pure protocol: wire types (serde), version guard, key scheme, reference rate codec, action-plane safety governor, in-process `Bus`/`LocalBus` and control loop. `serde`-only — no transport, no async. |
| **`ncp-zenoh`** | The recommended **decoupled** transport: Zenoh *queryable* (control-plane RPC) + *pub/sub* (perception/action/observation data planes), each with the QoS its job needs (see [`Plane`]). |
| **`ncp-gateway`** | **Engram's Rust edge**: runs the Zenoh bus and bridges control-plane RPC to the Python `SessionService` over a localhost socket (NEST stays Python). |
| **`ncp-python`** | **Python binding** (PyO3): the Rust core (version guard, key scheme, codec, safety, message validation) as an importable `ncp` module — so Python peers are wire-identical without reimplementing. |
| **`ncp-cpp`** | **C / C++ binding**: a stable C ABI (`extern "C"` + `include/ncp.h`) over the same core, so C and C++ projects link `ncp_cpp` instead of reimplementing the wire. |

## One Rust core, four languages

NCP is written in Rust and **works from Python, TypeScript and C++** off the
*same* core, so every peer is wire-identical:

- **Rust** — depend on `ncp-core` (+ `ncp-zenoh`) directly.
- **Python** — `import ncp` (the `ncp-python` PyO3 extension): `ncp.Keys`,
  `ncp.check_version`, `ncp.encode_rates`/`decode_command`, `ncp.govern`,
  `ncp.validate`. Build with `maturin develop -m ncp-python/Cargo.toml`.
- **TypeScript** — the canonical types in `ncp-core/bindings/*.ts` are **generated
  from the Rust types** by ts-rs (`cargo test -p ncp-core --features ts`); import
  them and keep your transport (WebSocket / Tauri-Zenoh) in TS. (Zenoh is native,
  so the *transport* doesn't compile to browser WASM; the *types* come from Rust.)
- **C / C++** — include `ncp-cpp/include/ncp.h` and link `ncp_cpp`
  (`cargo build -p ncp-cpp`); see `ncp-cpp/examples/demo.cpp`.

### Docs
- [`RATIONALE.md`](RATIONALE.md) — why NCP exists, unbiased vs the alternatives.
- [`INTEGRATING.md`](INTEGRATING.md) — per-language quickstart + 10-adopter-lens evaluation (simple, minimally invasive).
- [`NEST_REALTIME.md`](NEST_REALTIME.md) — can NCP read NEST live without stopping it (like MUSIC)? Yes; 10-way analysis.
- [`PERFORMANCE.md`](PERFORMANCE.md) — does NCP bottleneck NEST? The one real bottleneck (recorder readback) found + fixed; per-tick cost model.
- [`RESILIENCE.md`](RESILIENCE.md) — robustness over a poor/jammed link (packetized predictive control, fail-safe, and where Partial Information Decomposition fits), pruned to what's worth building. Primitives shipped in `ncp-core`: `ActionBuffer` (predictive replay + ttl HOLD), `CommandWatchdog`, `LinkMonitor` (seq-gap + CUSUM) + `LinkStatus`.
- [`NEUROMORPHIC.md`](NEUROMORPHIC.md) — NCP as the stable interface for neuromorphic hardware (Loihi/Lava, SpiNNaker/PyNN, BrainScaleS) and the sim-before-deploy / differential-testing workflow.
- [`PLASTICITY.md`](PLASTICITY.md) — single neuron / population / custom-parameter neurons / multimeter, and long- + short-term + reward-modulated plasticity driven by plant (UAV) feedback (`Observable::Weight`, the reward stimulus channel).

**Multiple UAVs, varying sensors/actuators:** one session per UAV; each named
sensor/actuator on its own sub-key (`…/session/{uav}/sensor/{name}`,
`…/command/{name}`); Engram taps a UAV's whole set with `…/sensor/**` and the fleet
with `{realm}/session/**` (`Keys::sensor_glob`/`command_glob`/`fleet_glob`,
`ZenohBus::put_sensor_named`/`subscribe_command_named`/`subscribe_fleet`). `seq` is
per-entity-stream; instantiate one `LinkMonitor`/`ActionBuffer` per entity.

## The three planes

Perception and action are **separate planes** — opposite-signed on rate, payload
size, fan-in/out, failure isolation and safety authority — so they ride separate
keys with separate QoS. The control-plane RPC (session lifecycle) is a fourth,
rare, request/reply key. The **NEST brain is *not* split**: a closed sensorimotor
loop is one `nest.Run(chunk)` binding sense→act; only the wire diverges.

```text
{realm}/rpc                              control-plane RPC   queryable, reliable
{realm}/session/{id}/sensor[/{name}]     perception plane    pub/sub, DROP, conflate to latest (lossy-OK)
{realm}/session/{id}/command[/{name}]    action plane        pub/sub, express + DROP + RealTime, safety-gated (ttl/HOLD/ESTOP)
{realm}/session/{id}/observation         neural / diagnostic pub/sub — free read-only observer tap (e.g. pid_vla)
```

The pub/sub data planes mean **observers attach for free**: an analyzer (pid_vla)
subscribes read-only to `…/sensor` / `…/command` / `…/observation` with zero
changes to the control path — the structural reason to choose a data-centric bus
over point-to-point gRPC for a fleet + watchers.

## Quick start

```rust
use ncp_core::{OpenSession, NetworkRef, NetworkRefKind, RecordSpec, RecordTarget, Observable};
let open = OpenSession {
    session_id: "uav3-percept".into(),
    network: NetworkRef { kind: NetworkRefKind::Builtin, ref_: "iaf_psc_alpha".into(),
        population_sizes: [("feat".into(), 1)].into_iter().collect(), ..Default::default() },
    record: RecordSpec { targets: vec![RecordTarget {
        port: "spk".into(), target: "feat".into(), observable: Observable::Spikes, ..Default::default() }] },
    ..Default::default()
};
// serde_json::to_string(&open) is wire-identical to the Python/TS clients.
```

Over Zenoh (async):

```rust
use ncp_zenoh::{ZenohBus, ZenohNcpClient};
let bus = ZenohBus::open().await?;             // realm engram/ncp
let client = ZenohNcpClient::new(bus.clone());
let opened = client.open(&open).await?;        // control-plane RPC (queryable)
bus.put_sensor("uav3", &serde_json::to_vec(&sensor_frame)?).await?;  // perception plane
bus.subscribe_commands("uav3", |_k, bytes| { /* decode CommandFrame → actuator */ }).await?;
```

## Run the Engram gateway (NEST stays Python)

```bash
# 1) Python side: the NCP server (SessionService + NestBackend) over a localhost socket
conda run -n p2b python -m backend.neurocontrol.bridge_server --backend nest

# 2) Rust side: the Zenoh edge that fronts it
cargo run -p ncp-gateway      # NCP_REALM, NCP_BRIDGE_ADDR=127.0.0.1:28474 configurable
```

## Cross-repo consumption

NCP is **one canonical crate** that every peer depends on (a standard has one
implementation, not vendored copies). The consumers in sibling repos use a path
dependency to this workspace:

```toml
# crebain/src-tauri/Cargo.toml   (optional, behind the `ncp` feature)
ncp-core  = { path = "../../Paper2Brain/ncp/ncp-core",  optional = true }
ncp-zenoh = { path = "../../Paper2Brain/ncp/ncp-zenoh", optional = true }

# pid_vla/crates/ncp-observer/Cargo.toml
ncp-core  = { path = "../../../Paper2Brain/ncp/ncp-core" }
ncp-zenoh = { path = "../../../Paper2Brain/ncp/ncp-zenoh" }
```

The sibling layout is the local-development contract. For external adopters or
standalone CI, switch these to a `git`/`crates.io` dependency — a one-line change
that needs no code edits (the crate is self-contained).

## Build & test

```bash
scripts/check.sh             # full conformance/smoke matrix (all crates + bindings)

cargo test -p ncp-core       # pure, fast (serde only) — wire-compat + codec + safety + loop
cargo test -p ncp-core --features ts          # (re)generate the TypeScript types
cargo test -p ncp-zenoh --test loopback       # real Zenoh runtime: streaming control loop
cargo build -p ncp-gateway   # the Engram edge binary
cargo build -p ncp-cpp       # the C/C++ ABI (+ ncp-cpp/include/ncp.h)
```

## Scientific boundary (binding)

Returned `V_m`/spikes are **raw simulation outputs of a specified model**, never a
validated reproduction: every `ObservationFrame` carries `calibrated_posterior=false`
and `is_simulation_output=true`. A neuro-controller is a **control artifact**,
never a paper-reproduction claim.

## License

MIT.
