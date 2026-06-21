# Neuro-Cybernetic Protocol (NCP) v0.4

A versioned, **transport-agnostic, project-agnostic** standard for letting a
running NEST simulation serve external robot / UAV / simulation systems —
robot/UAV bodies, analysis/observer clients, and **any others** ("there could be
more"). One protocol, two complementary interaction patterns:

1. **Neural-simulation service** *(the general case)* — an external system **asks
   the simulation backend for a simulation**, declares **what and where to record**
   (membrane potential / spikes / rate from a single neuron, a synapse, or a
   population) and **what stimuli to inject**, then **steps or runs** the simulation
   and reads back the neural data. It serves **perception, action, both, or neither**:
   the backend runs only the neural part the client requests; whether a given
   sensor/actuator is NEST-backed or classic ML is entirely the client's choice.
2. **Closed-loop controller** *(a layered special case)* — the neural backend drives
   an external actuator as "just another controller" over the system's *existing*
   transport (e.g. a robot/UAV client's MAVROS setpoint topics), non-invasively.

Normative wire contract: `proto/ncp.proto` (proto-native; the JSON Schemas are its
JSON projection). The wire is **simulator-agnostic** — the typed record/stimulus
vocabulary are abstract SNN concepts a `SimulationBackend` maps to its simulator
(NEST today; NEURON/Brian2/GeNN are a future *backend*, no wire change — see
[`ROADMAP.md`](ROADMAP.md)). Reference implementations:
- **Rust (reference implementation):** this Rust workspace — [`ncp-core`](ncp-core/)
  (pure protocol: wire types, version guard, key scheme, rate codec, safety
  governor, in-process bus + control loop), [`ncp-zenoh`](ncp-zenoh/) (the decoupled
  Zenoh transport with per-plane QoS), and [`ncp-gateway`](ncp-gateway/) (the
  simulation host's Rust edge — see §6A). NCP is intended to become a reusable
  standard (cf. MCP/ACP); Rust is the high-performance reference implementation,
  self-contained and extractable to its own repo / crates.io. **Language bindings
  off the same core:** Python (`ncp-python`, PyO3), TypeScript (`@sepehrmn/ncp`,
  ts-rs-generated types), and C/C++ (`ncp-cpp`, a C ABI + `ncp.h`) — every peer is
  wire-identical. Integration is documented in [`INTEGRATING.md`](INTEGRATING.md);
  real-time NEST interaction vs MUSIC in [`NEST_REALTIME.md`](NEST_REALTIME.md).
- **Python:** the host simulation service — a NEST-driving `SessionService` +
  `NestBackend` and an in-process reference client (the reference deployment runs
  this behind the Rust gateway).

Machine-readable contract (proto-native): the protobuf IDL `proto/ncp.proto` is
the **normative wire contract** — the single source of truth for message structure
and the binary encoding. The **JSON Schemas** `schemas/*.schema.json` are its JSON
projection (parity-guarded by `scripts/check_proto_schema_parity.py`), and the
Rust/Python/TS bindings generate from or conform to it via buf. All reference
implementations serialize to the **same** wire, so they interoperate. This
document is the human-readable spec.

> **Why NCP exists** (unbiased rationale vs ROS 2/DDS, Zenoh, MUSIC, the
> Neurorobotics Platform, MCP/ACP, gRPC, dm_env_rpc, and the "compose, don't
> invent" alternative): [`RATIONALE.md`](RATIONALE.md).

> **Scientific boundary (binding).** Returned `V_m`/spikes are **raw simulation
> outputs of a specified model**, never a validated reproduction. Every
> simulation response carries `calibrated_posterior=false` and
> `is_simulation_output=true` and references a backend-issued **handle**, not raw
> code or a path. A neuro-controller is a **control artifact**, never a
> paper-reproduction claim. Engram's existing safety/handle discipline applies
> unchanged.

## 1. Versioning & compatibility

Every message carries `ncp_version` (semver). Consumers **ignore unknown fields**, so
adding an *optional* field or a new message type is **non-breaking** and does not bump
the version (since v0.4). An **incompatible** change (removing/retyping/renaming a
field, removing an enum value) is breaking; pre-1.0 the **minor is breaking** for those
— a receiver checks the full version and an exact `(major, minor)` match is required,
any `0.x` minor difference is fail-closed rejected, never coerced.

Two layers separate *compatibility* from *identity*: `ncp_version` is the hard
compatibility gate (above), while `contract_hash` (carried in
`OpenSession`/`SessionOpened`; `ncp_core::CONTRACT_HASH`, FNV-1a of the
wire-semantically-canonicalized proto) is an **advisory** identity signal — a
mismatch within a compatible version is *logged, not rejected* (the peers are on
different but compatible contract revisions). A strict `verify_contract` opt-in
exists for deployments that mandate an exact revision. NCP is **0.4** — pre-1.0, the
wire may still change; pin the exact version you build against.

## 2. Entity model (perception, action, neither; 0..N of each)

A client system has a hierarchy: a **system** (e.g. `uav1`) with **0..N
sensors** and **0..M actors** (e.g. UAVs), each actor itself having **0..N
sensors** and **0..K actuators**. NCP addresses entities by a string path and a
role, e.g.

```
uav1/sensor/cam0          role=sensor
uav1/actuator/rotor       role=actuator
ground/radar0             role=sensor
```

Entities are bound to **named ports** of a simulation (a `stimulus` port or a
`record` port) via `EntityBinding`. The binding is the client's concern; Engram
only sees ports. This is what makes the protocol agnostic to how many sensors or
actors exist (including zero) and reusable across projects.

## 3. The neural-simulation service

A **session** is one running simulation with a declared recording and stimulus
surface. Lifecycle (each message has a JSON Schema of the same name):

| Message | Dir | Purpose |
|---|---|---|
| `open_session` | client → server | request a simulation: a `NetworkRef`, a `RecordSpec`, a `StimulusSpec`, a `SimConfig`, optional entity `bindings` |
| `session_opened` | server → client | ack with backend, resolved population sizes, and `SimProvenance` (model/seed, `calibrated_posterior=false`, `is_simulation_output=true`) |
| `step_request` | client → server | advance one chunk; optional `stimulus_frame`; returns an `observation_frame` |
| `run_request` | client → server | batch: advance `duration_ms` holding a stimulus; returns an `observation_frame` |
| `observation_frame` | server → client | recorded data per record port (see below) |
| `close_session` / `session_closed` | both | tear down |

### NetworkRef — what to simulate
- `kind=handle` — a backend-issued `pynest_script_id` / `compiled_module_id` (a
  backend-generated network; the canonical, handle-based path).
- `kind=builtin` — a NEST built-in neuron model name (e.g. `iaf_psc_alpha`) with
  `population_sizes` (quick single-neuron / population sims).
- `kind=model_id` — a knowledge-graph / paper-derived model id.
- `kind=spec` — a small inline spec (advisory).
- `model_name` (optional) selects which registered model to create for a multi-model
  `handle`; `params` (numeric) / `population_sizes` carry advisory overrides.

### RecordSpec — what & where to record
A list of `RecordTarget { port, target, observable, ids[], cadence_ms, recordables[] }`:
- `port` — the client's name for this recording (keys the observation).
- `target` — population / group name in the network.
- `observable` — `V_m` | `spikes` | `rate` | `weight` | `binary_state` (the last
  for binary/multi-state neurons, recorded via a spin detector, not a multimeter).
- `ids` — neuron/synapse indices (empty = all in `target`).
- `cadence_ms` — analog sampling interval (ignored for `spikes`).
- `recordables[]` — generic named, model-specific recordables beyond the typed
  `observable` (e.g. `g_ex`/`g_in` for conductance models, `w` for adaptation,
  `rate` for rate models), resolved via the backend's multimeter `record_from`.
  Empty = just `observable`. (#10)

### StimulusSpec / StimulusFrame — what to inject
A list of `StimulusTarget { port, target, kind, ids[], params{} }` declares the
input ports; each `step`/`run` carries a `StimulusFrame { values: {port → ChannelValue} }`.
`kind` ∈ `current_pA` | `rate_hz` | `spike_times` | `weight_set` | `rate_inject`
(continuous-rate injection for rate-based neurons via rate connections /
`step_rate_generator` — rate models cannot receive spikes). `params{}` carries
named scalars beyond the value, e.g. a siegert neuron's `drift_factor` /
`diffusion_factor`. (#10) A `ChannelValue` is `{ data: float[], unit }` — e.g.
`[500.0]` pA, `[40.0]` Hz, or a list of spike times.

### ObservationFrame — the returned neural data
`records: { port → Observation }`, where `Observation` is
`{ port, target, observable, times[], values[], senders[], unit, recordable }`:
- analog (`V_m`): `times` (ms) + `values` (mV), parallel.
- `spikes`: `times` (spike times, ms) + `senders` (neuron ids), parallel.
- `rate`: `values=[rate_hz]`.
- `recordable` — when set, names which specific recorded series this carries
  (e.g. `g_ex`, `w`) and is authoritative for it; `observable` is then the record
  target's family hint, not this series' type. `""`/absent = the series is
  `observable`. (#10)

This is exactly "pass stimuli → get back membrane potential, conductance, spiking,
binary-state, or rate data from a single neuron / synapse / population".

**Bulk option (#6).** For large spike trains / `V_m` traces, the observation plane
may additionally carry a `bulk_observation` frame — the same metadata plus a packed
little-endian column block (`ncp-core::bulk`; proto `BulkObservation`) instead of
`repeated double`/`int64`: parse-free, random-access, ~2× smaller. It is an
**additive, negotiated** option on the observation/analysis plane only (never the
hot action loop); the JSON `ObservationFrame` above stays the canonical
representation. See [`PERFORMANCE.md`](PERFORMANCE.md).

## 4. The closed-loop controller (layered)

For the "the neural network is the brain" pattern, NCP adds control messages:
`capabilities` (handshake), `sensor_frame` (plant → controller), `command_frame`
(controller → plant), `control_status`. A **codec** (`CodecSpec`) declares the
sensor→spike encoding and spike→command decoding so a trained SNN trains against
a frozen interface. The controller loop is `sensor_frame → encode → stimulus →
step(chunk) → record → decode → command_frame`, i.e. it is built *on* the session
service. `SafetyLimits` bound commands and a stale sensor forces `HOLD`.

## 5. Transport bindings (and why)

NCP separates the **contract** from the **medium**. The contract is proto-native:
the **protobuf IDL** `proto/ncp.proto` is the normative source of truth, and the
**JSON Schemas** in `schemas/` are its JSON projection (kept in parity, CI-guarded).
The medium is a per-deployment choice behind the `Transport` abstraction — do
**not** marry NCP to one wire. With many heterogeneous projects this matters; the
trade-offs:

The key lens is **coupling**: with dozens of loosely-coupled systems you do not
want each client wired to a server address.

| Medium | Coupling | Upsides | Downsides | Use when |
|---|---|---|---|---|
| **Zenoh** — *recommended decoupled default* | **low** — addresses *data* (`{realm}/**`), automatic discovery, many-to-many | RPC via **queryable**, streaming via **pub/sub**; location-transparent; N server instances on one keyspace; **robot/UAV clients already speak it** (a `ZenohBridge`, ROS 2 `rmw_zenoh`); carries protobuf or JSON | younger RPC ecosystem; you define the queryable convention; browsers need a router's WS plugin | the many-project fleet; robotics-native; multiple/replicated server instances |
| **WebSocket + JSON** — *zero-friction fallback* | medium (client → one URL) | works from any language incl. browsers/Tauri-webview; human-readable; no codegen | no typing/codegen; manual correlation; verbose at high rate | quick starts, debugging, the frontend (shipped: `/api/neurocontrol/ws`) |
| **gRPC** (HTTP/2 + protobuf) — *optional point-to-point* | **high** — client dials a host:port; needs a load balancer to scale | first-class bi-di streaming; typed codegen from `ncp.proto`; deadlines/backpressure | endpoint coupling; browser needs grpc-web/Connect; protoc step | cloud/enterprise point-to-point with a known endpoint |
| **ROS 2 (DDS) + rosbridge** | low (within ROS) | native for ROS projects; QoS; rosbridge bridges browsers | couples non-ROS projects to ROS; heavy | the project is already ROS 2 |
| **NATS / MQTT / ZeroMQ** | varies | fast pub/sub (+ NATS req-reply); ubiquitous (MQTT) | weaker typing/RPC; reinvent framing | existing message-bus deployments |

**Decision (proto-native; see [`VERSIONING.md`](VERSIONING.md) and [`RATIONALE.md`](RATIONALE.md)):** treat
`proto/ncp.proto` as the **normative wire contract** (the source of truth) with the
**JSON Schemas** as its parity-guarded JSON projection; make **Zenoh the recommended *decoupled* default** for the
bus (RPC via queryable, streaming via pub/sub — so no client is bound to a server
address); keep **WebSocket/JSON** as the no-dependency fallback (shipped, and what
a browser/Tauri-based client's frontend uses); treat **gRPC** as an *optional* point-to-point binding
for deployments that specifically want it. The bus binding is `bus.py`
(`Bus`/`LocalBus`/`ZenohBus` + `NcpBusServer`/`NcpBusClient`);
`SessionService.handle_json(message)` is the
transport-neutral seam every binding calls.

## 6. How a project integrates — and why the commander core stays project-agnostic

**Separation of concerns is the load-bearing rule:** project specifics (topic
names, message types, field layouts, transport deps) must **not** live in the
commander's repo (e.g. Engram) — it has to scale to dozens of projects. The
commander core speaks **only** NCP (entity/channel-addressed). A project integrates
via one of three mechanisms, in decreasing preference, all of which keep its
specifics *out of the commander*:

1. **Client-side adapter (best).** The project owns its NCP client **and** its
   mapping in *its own* repo/language, and calls the commander's service. For control,
   **the commander emits NCP `command_frame`s; the project's adapter maps them to its
   actuators** — so even the control path carries no project specifics in the commander.
   A robot/UAV client's drop-in adapter is a small client module (copy into your
   own `src/neuro/`; touches no existing client code).
2. **Declarative profile (data, not code).** The project ships a JSON mapping
   file that a *generic* loader (`profiles.DeclarativeProfile` via
   `load_profile(path, ns=…)`) consumes — no per-project class in core. A
   client's mapping lives as **data** in a declarative profile JSON, not as code.
3. **Plugin package (entry points).** Richer logic ships as a separately
   installable `engram-ncp-<project>` package registering a profile under the
   `engram.ncp.profiles` entry-point group (`profiles.discover_plugins()`).

The commander never assumes a fixed sensor/actor count: a system with no cameras or no
UAVs simply declares no ports; one with many addresses each by entity path.
Perception and action are symmetric — both are bindings of client entities to
stimulus/record ports. The core registry ships **only** the `generic` profile;
every concrete project is loaded from data or a plugin.

## 6A. The Rust edge: planes, QoS, and the gateway

The Rust SDK makes the transport decision concrete. **Perception and action are
separate planes** — opposite-signed on rate, payload size, fan-in/out, failure
isolation and safety authority — so they ride separate keys with separate QoS.
The **NEST brain is not split**: a closed sensorimotor loop is one
`nest.Run(chunk)` binding sense→act; only the wire diverges.

```text
{realm}/rpc                              control-plane RPC   queryable, reliable
{realm}/session/{id}/sensor[/{name}]     perception plane    pub/sub, best-effort DROP (lossy-OK)
{realm}/session/{id}/command[/{name}]    action plane        pub/sub, express + DROP + RealTime, safety-gated
{realm}/session/{id}/observation         neural / diagnostic pub/sub — free read-only observer tap
```

Per-entity sub-keys (`…/sensor/imu`, `…/command/cmd_vel`) address the
multi-sensor / multi-actuator case; a subscriber wildcards `…/sensor/**`. The
action plane is the only one with command authority (`ttl_ms`, `Mode.HOLD`/
`ESTOP`, `command_timeout_ms`, geofence). A `CommandFrame.seq` echoes the
`SensorFrame.seq` it was computed from, so a split-plane observer joins **on
`seq`, not arrival time** (the DROP QoS on the perception plane makes arrival-time
pairing unsound).

NCP's per-plane QoS **maps onto** the standard ROS 2 / DDS QoS vocabulary, so a
`rmw_zenoh` / DDS deployment can express the same contract. What the Zenoh binding
sets **today** is the Reliability/priority column only (`CongestionControl` +
priority + `express`); the History / lifespan / deadline columns are the DDS QoS
that would express the *same intent* but are **not currently configured on the
Zenoh wire** (and `ttl_ms` is enforced plant-side by `CommandWatchdog`, not as a
wire lifespan):

| NCP plane | Reliability (set today) | History (DDS mapping, not set today) | NCP safety field | ROS 2 / DDS equivalent |
|---|---|---|---|---|
| perception (sensor) | best-effort (DROP), DataHigh | _(would be `KEEP_LAST(1)`)_ | — | `BEST_EFFORT` |
| action (command) | best-effort (DROP), express, RealTime | _(would be `KEEP_LAST(1)`)_ | `ttl_ms` (plant-side) | `BEST_EFFORT`; `LIFESPAN`+`DEADLINE` would express `ttl_ms`/staleness |
| control RPC / observation | reliable (BLOCK) | keep-all | — | `RELIABLE` |
| liveness / fail-safe | — | — | `Mode.HOLD`/`ESTOP`, `command_timeout_ms` | `LIVELINESS` + `DEADLINE` watchdog (plant-side) |

`ttl_ms` is the application-layer analogue of DDS `LIFESPAN` (enforced plant-side,
not on the wire); the genuinely NCP-specific part is only the `mode`
enum as an explicit wire authority. Mapping to these names keeps NCP interoperable
with a ROS 2/DDS stack rather than diverging from it.

**Conformance — action-plane liveness (normative).** Because the action plane is
best-effort and **MAY** silently drop a `CommandFrame`, a conformant plant **MUST**
enforce command liveness locally: once the most-recent command's `ttl_ms` has
elapsed (measured on the plant's own clock), it **MUST** fail safe — HOLD to a safe
setpoint (zeroed / `Mode.HOLD`) — and **MUST NOT** continue actuating on the stale
setpoint or replay a horizon past its `ttl_ms`. The wire layer only *detects* a
gap (as DDS `DEADLINE` notifies but does not act); the plant owns the safe state —
the required "safe state" / de-energize-to-safe principle of functional-safety
practice (IEC 61508 / ISO 13849). A stale, duplicate, or out-of-order command
**MUST NOT** refresh the liveness deadline. NCP's reference plant-side primitives
are `CommandWatchdog` and `ActionBuffer` (see [`RESILIENCE.md`](RESILIENCE.md)). The
key words **MUST**, **MUST NOT**, and **MAY** are used as defined in
[RFC 2119](https://www.rfc-editor.org/rfc/rfc2119.html) and
[RFC 8174](https://www.rfc-editor.org/rfc/rfc8174.html) (only the uppercase forms
carry the normative meaning).

**The reference gateway (`ncp-gateway`).** When the commander's brain is NEST (Python)
— as in the Engram reference — its NCP *server* stays Python. The gateway gives it a production-grade Rust Zenoh edge
— it runs the `{realm}/rpc` queryable and the pub/sub planes and forwards each RPC
to the Python `SessionService` over a localhost socket, reusing the one
transport-neutral seam `handle_json`. NEST never leaves Python; the fleet-facing
transport is Rust:

```text
 Zenoh bus ──(SHM/QoS)──► ncp-gateway (Rust) ──(localhost JSON)──► bridge_server.py → SessionService → nest.Run
    ▲ robot/UAV bodies / analysis-observer clients / dashboards attach as peers / observers
```

```bash
# reference deployment: run the host simulation service (NEST) behind the gateway
python -m <host>.bridge_server --backend nest   # the Python NEST host
cargo run -p ncp-gateway                         # the Rust edge, from the workspace root
```

## 7. Status & roadmap

Implemented: the protocol (+ `ncp.proto` IDL + JSON Schemas); the **project-agnostic**
profile mechanism (generic + `DeclarativeProfile` data loader + plugin discovery;
a client's mapping carried as **data** in a declarative profile JSON); a deterministic
**MockBackend** and a **real `NestBackend`** (live NEST 3.9 — V_m/spikes via
multimeter/spike_recorder, `Prepare/Run/Cleanup`) for **both built-in models and
`kind=handle`** (a generated/compiled Engram network resolved via the artifact
store: `load_compiled_module` → install the absolute `.so` → create the registered
model); the `SessionService` + reference client; the **WebSocket/JSON** binding
(`/api/neurocontrol/ws`); the **decoupled bus** binding (`bus.py`: `LocalBus` tested
+ lazy `ZenohBus`, RPC-via-queryable + pub/sub streaming); the `SessionController`
(a spiking session *is* the controller); a client-side TS client; and a reflex
closed-loop demo.

Also implemented: the **Rust reference SDK** (`ncp/`) — `ncp-core` (wire types,
version guard, key scheme, rate codec, safety governor, in-process bus + control
loop; unit-tested for wire-compatibility with the Python JSON), `ncp-zenoh` (the
Zenoh transport with per-plane QoS), and `ncp-gateway` + `bridge_server.py`
(Engram's Rust edge bridging to the Python `SessionService`). Two kinds of consumer
wire against it: a **robot/UAV client** (a self-contained client module behind an
`ncp` feature — a native Rust+Zenoh client and a pose/velocity↔NCP mapping) and an
**analysis/observer client** (a read-only tap mapping the data planes to an external
analysis representation, joining on `seq`).

Also implemented since: the **streaming control plane over Zenoh**
(`ncp_zenoh::ZenohControlTransport` + `ncp_core::NeuroControlLoop` — `sensor`→
`command` over pub/sub, no per-tick RPC; verified by a real-Zenoh loopback test);
`nest.Run` **offloaded off the WebSocket event loop** (`backend/api/neurocontrol.py`,
single shared worker thread); `ObservationFrame.seq` for exact split-plane
`(V,L,D,A)` alignment; the **C/C++** (`ncp-cpp`) and **Python** (`ncp-python`)
bindings; and a conformance/smoke script (`ncp/scripts/check.sh`). Plus the
**degraded-link resilience** primitives (`ncp-core`: `ActionBuffer` predictive
replay, `CommandWatchdog` enforcing `ttl_ms`, `LinkMonitor` seq-gap+CUSUM →
`LinkStatus`, `CommandFrame.horizon`); **multi-UAV / varying sensor-actuator**
addressing (`Keys::sensor_glob`/`command_glob`/`fleet_glob`, per-named-entity
`ZenohBus` methods); the **NestSession O(history) readback bottleneck fix**; and
docs `PERFORMANCE.md`, `RESILIENCE.md`, `NEST_REALTIME.md`, `NEUROMORPHIC.md`,
`INTEGRATING.md`.

Scaffolded / next: **action-plane auth/ACL** — a default-deny per-plane Zenoh ACL
template + TLS/ACL enablement steps now ship (#7; `deploy/zenoh-access-control.json5`,
`SECURITY.md`), with live mTLS-enforcement validation the remaining P0; a `no_std`
core + tiny transport
(zenoh-pico / micro-ROS) for MCUs; per-session capability negotiation; spike-time/
weight stimuli; multi-population/multi-model handles; an optional **gRPC** binding
from `ncp.proto`; a conformance program + neutral spec home for the standard (see
[`GOVERNANCE.md`](GOVERNANCE.md)); and a trained SNN-RL controller.
