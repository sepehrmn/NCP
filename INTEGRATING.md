# Integrating NCP into a new project

NCP is designed so adopting it is **simple and minimally invasive** — and, for the
"a NEST point/rate neural network served live to external clients" use case, better than the
alternatives (the honest comparison, including where it is *not*, is in
[`RATIONALE.md`](RATIONALE.md)).

## Topology: the roles a deployment wires together

NCP itself is **generic** — it bakes in no consumer, no robot, no brain (see
[`README.md`](README.md) and [`RATIONALE.md`](RATIONALE.md)). What a *deployment*
does with the contract is wire independent peers into a few recurring roles. The
reference fleet looks like this, and understanding it first makes the rest of this
guide concrete:

- **Hub / command-center — Engram.** Engram runs the NEST spiking-simulation
  backend and is the *commander*. Crucially, it drives robots and UAVs
  **interchangeably through NCP**: to Engram every body is just a `session` exposing
  the same perception/action planes, so the same controller steers a simulated quad,
  a real UAV, or another NEST model without special-casing any of them. Engram's Rust
  edge is `ncp-gateway`, which terminates the Zenoh bus and bridges control-plane RPC
  to the Python NEST host (`bridge_server.py`) over a localhost socket — NEST stays in
  Python while the wire stays language-neutral.
- **Body / tactical app — crebain**, a tactical-UAV application. crebain is
  deliberately **"both or either"**: it runs **standalone** on its own self-contained
  drone stack (the Tauri app + MAVROS, no NCP at all), **and/or** it joins a fleet over
  NCP — the two paths are not mutually exclusive. NCP support is a self-contained,
  **off-by-default** `ncp` Cargo feature living in `src-tauri/src/ncp/`, so the
  standalone build *and its command-contract test* are byte-for-byte unchanged whether
  or not the fleet path is compiled in. With the feature on, crebain publishes its pose
  on the perception plane and is steered by `CommandFrame`s on the action plane,
  failing safe to zero velocity on `hold`/`estop`.
- **Observer — [prisoma](https://github.com/sepahead/prisoma)** (and similar). A
  read-only consumer that *taps* the observation plane (and optionally perception) and
  **drives nothing** — it never publishes the action plane.

The whole point of the contract is exactly this interop: Engram-the-hub neither knows
nor cares whether a `session` is crebain's simulated quad, a real airframe, or a NEST
network — they are addressed, versioned, and safety-gated identically. Adding a new
body or observer is just a new peer pinning the wire tag; it changes **zero** hub code
(the minimal-invasiveness contract below).


## The minimal-invasiveness contract

1. **The commander stays project-agnostic.** The NCP commander/backend (e.g. Engram)
   speaks only NCP (entity/channel-addressed messages). It carries **no** project
   topic names, message types, or field layouts. So adding your project changes
   **zero** commander code.
2. **Your project owns its mapping, in your repo, in your language.** A thin
   adapter maps your sensors/actuators ↔ NCP `SensorFrame`/`CommandFrame`/stimulus/
   record. That is the *only* code you write.
3. **It bolts on without touching what you have.** A robot/UAV client's
   integration can be a self-contained `src/ncp/` module behind an off-by-default
   `ncp` Cargo feature — the default build and its command-contract test stay
   unchanged. An analysis/observer client's is a read-only observer crate that
   drives nothing. Neither needs to edit existing code.

## Registering a consumer (zero NCP-repo changes)

The principle above extends to the **release tooling**: onboarding a consumer must
not require editing the NCP repo. NCP names no consumer — its pin tooling
(`scripts/check-consumer-pins.sh`, `scripts/repin-ncp.sh`) **discovers** consumers by
globbing sibling repos for a `.ncp-consumer` descriptor. You register by committing
that file to **your own** repo root; NCP never changes.

`.ncp-consumer` is a small line-oriented file (`#` comments) declaring which of your
files carry the NCP pin and, optionally, how to re-pin a bespoke layout:

```text
# how this repo pins NCP — read by NCP's generic, consumer-agnostic tooling.
cargo_tag   src-tauri/Cargo.toml     # ncp-core/ncp-zenoh git-dep `tag = "vX"`
cargo_lock  src-tauri/Cargo.lock     # resolved `NCP?tag=vX`
npm_tag     package.json             # `"@…/ncp": "github:…/NCP#vX"`
npm_lock    bun.lock                 # same spec `#vX` (+ resolved commit)
mirror_ref  ncp/.mirror-ref          # a vendored-mirror pin file (the tag string)
repin_cmd   scripts/sync_mirror.sh {TAG}   # OPTIONAL consumer-owned re-pin ({TAG} substituted)
```

Declare only the lines that apply to you (a pure observer might declare just
`cargo_tag`/`cargo_lock`; a vendored mirror declares `mirror_ref` + a `repin_cmd`
that runs its own sync). `check-consumer-pins.sh` then verifies every discovered
consumer pins one agreed tag, and `repin-ncp.sh <tag>` re-pins them all — both with
no NCP-side edit when a new consumer appears.

## 5-minute quickstart, per language (one canonical Rust core)

**Rust** — depend on the SDK; subscribe/publish the planes or open a session:
```rust
let bus = ncp_zenoh::ZenohBus::open().await?;                 // realm "ncp" default; for a deployment realm: open_realm(Keys::new("engram/ncp")) — see "Realm & keys" below
bus.subscribe_commands("uav1", |_k, b| { /* CommandFrame → your actuator */ }).await?;
bus.put_sensor("uav1", &serde_json::to_vec(&sensor_frame)?).await?;
// or, to be the controller: ncp_zenoh::ZenohControlTransport + ncp_core::NeuroControlLoop
```

**Python** — `import ncp` (the PyO3 module); speak JSON to the gateway/WS:
```python
import ncp                       # canonical keys/codec/version/validation in Rust
k = ncp.Keys()                   # k.sensor("uav1"), k.command("uav1"), ...
cmd_json = ncp.decode_command(codec_json, rates_json, t=0.0, seq=7)
```

**TypeScript** — import the Rust-generated types; keep your WS/Tauri transport:
```ts
import type { SensorFrame, CommandFrame, ObservationFrame } from "ncp/bindings";
// send/receive the same JSON over /api/neurocontrol/ws or your ZenohBridge
```

**C++** — include `ncp.h`, link `ncp_cpp`:
```cpp
#include "ncp.h"
char* key = ncp_key_command("ncp", "uav1");   // ncp_string_free(key)
char* cmd = ncp_decode_command(codec_json, rates_json, 0.0, 7,
                               /*frame_id=*/NULL, /*mode=*/NULL);
```

In every language the *behavior* (key scheme, codec, version guard, safety,
validation) comes from the one Rust core, so all peers are wire-identical.

## Realm & keys — how peers find each other on the bus

Every NCP message rides a Zenoh **key expression** built from one `{realm}` prefix
plus the plane. The realm is **addressing, not a credential**: every peer in a
deployment agrees on the same realm string so their keyspaces line up — a wrong realm
just means you never meet, it grants nothing (for actual authorization see the
per-plane ACL + mTLS in [`SECURITY.md`](SECURITY.md)).

```text
{realm}/rpc                              control-plane RPC   (queryable; Open/Step/Run/Close)
{realm}/session/{id}/sensor[/{name}]     perception plane    (pub/sub, best-effort DROP)
{realm}/session/{id}/command[/{name}]    action plane        (pub/sub, best-effort DROP + express; ttl_ms is plant-side, safety-gated)
{realm}/session/{id}/observation         observation plane   (pub/sub, free read-only tap)
```

The default realm is the neutral `"ncp"`. A deployment picks its own — an Engram
fleet standardises on `"engram/ncp"`, and every consumer (a crebain bridge, a prisoma
observer) targets that same string. You set it equivalently from any peer:

- **Rust SDK:** `ZenohBus::open_realm(Keys::new("engram/ncp"))`. Plain
  `ZenohBus::open()` uses the `"ncp"` default — it does **not** read `NCP_REALM`.
- **`ncp-gateway` and the examples:** the `NCP_REALM` env var (default `ncp`).
- **Other language peers:** `ncp.Keys("engram/ncp")` (Python),
  `ncp_key_command("engram/ncp", id)` (C/C++).

`{id}` names one session (e.g. `uav1`). Per-entity sub-keys (`…/sensor/imu`,
`…/command/cmd_vel`) extend a plane for the multi-sensor / multi-actuator case, and
subscribers wildcard with `**`: a controller subscribes `…/session/uav1/sensor/**`,
a fleet observer subscribes `{realm}/session/**`. Ids and names must be single key
segments — the key builders reject `/ * $ # ?` and whitespace **fail-closed**, so a
wildcard-bearing id can't leak across sessions.

### What's actually on the wire (so you size buffers and pick tooling right)

- **The runtime encoding is JSON.** The sensor, command and RPC planes ship
  `serde_json` payloads — human-readable, debuggable from any Zenoh/WS client, and the
  default everywhere. The end-to-end cost is tiny (~1 µs for a full control tick; see
  [`PERFORMANCE.md`](PERFORMANCE.md)), so JSON stays the default.
- **Bulk / observation data uses the binary `BulkBlock`.** Large numeric arrays
  (spike trains, V_m matrices) ride a compact little-endian columnar block, not JSON.
- **Protobuf (`proto/ncp.proto` + `gen/`) is the *schema contract*, not the shipped
  encoding.** It is the IDL / source-of-truth that pins field names and feeds the
  conformance corpus, and the TypeScript types derive from the same contract — but the
  prost Rust bindings are **not** compiled into any runtime path (`gen/rust` is not a
  workspace member and there is no prost runtime dependency). Treat the `.proto` as the
  spec you conform to, not bytes you parse. A negotiated protobuf encoding is a possible
  *opt-in* only if a kHz / bandwidth-constrained peer ever needs it.

## Run the examples (copy these; don't start from scratch)

NCP ships runnable, dependency-light examples that exercise the exact planes, keys and
safety gates a consumer integrates against. Read them before writing your adapter:

| Example | What it shows | Run |
|---|---|---|
| [`ncp-zenoh/examples/uav_drone_loop.rs`](ncp-zenoh/examples/uav_drone_loop.rs) | The **action plane** end-to-end over Zenoh: a controller publishes `CommandFrame`s; a minimal quad plant subscribes on `{realm}/session/<id>/command`, enforces the `mode` and `ttl_ms` safety gates locally, and writes a JSONL trajectory a body (e.g. the crebain browser drone) can replay. | `cargo run -p ncp-zenoh --example uav_drone_loop` |
| [`ncp-core/examples/uav_control_safety.rs`](ncp-core/examples/uav_control_safety.rs) | The full safety surface, deterministically and transport-free: closed-loop flight (`NeuroControlLoop` + `ReflexController`), every `SafetyGovernor` gate (speed clamp, geofence→latched ESTOP, stale-sensor HOLD, non-finite-clock fail-safe, horizon clamp), `ActionBuffer` replay through a dropout, and the `CommandWatchdog` ttl deadline. | `cargo run -p ncp-core --example uav_control_safety` |
| [`ncp-core/examples/overhead.rs`](ncp-core/examples/overhead.rs) | The overhead benchmark — per-tick JSON (de)serialization of the action/perception frames, the safety governor, the reflex controller, and `BulkBlock` vs JSON for the same payload. Run it before claiming NCP is "too much overhead" (full results in [`PERFORMANCE.md`](PERFORMANCE.md)). | `cargo run -p ncp-core --release --example overhead` |
| [`e2e/nest_five_networks.py`](e2e/nest_five_networks.py) | Five distinct **real NEST** spiking models driven through the NCP RPC contract (`open_session` → `step_request*` with `current_pA` stimulus and spike recording → `close_session`) against Engram's `bridge_server --backend nest`. | see [`e2e/README.md`](e2e/README.md) |

Because the key scheme, codec, version guard and safety all come from the one Rust
core, these examples behave identically when reproduced from the Python, TypeScript and
C++ peers.


## Picking the integration mechanism (decreasing preference)

1. **Client-side adapter** (best) — your NCP client + mapping in your repo, against
   `ncp-core`/`ncp.proto`/`schemas/` (your client's own `src/ncp/` adapter module).
2. **Declarative profile** (data, not code) — a JSON mapping a generic loader
   consumes; no per-project class in the commander (a declarative profile shipped by
   your client).
3. **Plugin package** — a `<commander>-ncp-<you>` package (e.g. `engram-ncp-<you>`)
   registering a profile via entry points.

## Evaluated from 10 adopter lenses — what each gets, what was missing, what changed

| # | Adopter | Gets | Was missing → status |
|---|---|---|---|
| 1 | **Robotics / UAV engineer** | action + perception planes, pose/velocity↔frame mapping, safety mode | streaming control loop over the bus → **added** (`ZenohControlTransport`) |
| 2 | **Computational neuroscientist** | live data from a *running* NEST kernel (persistent `Prepare`/`Run`), V_m/spikes/rate, stimulus injection | "is it real-time like MUSIC?" → **answered** ([`NEST_REALTIME.md`](NEST_REALTIME.md)); multi-sim coupling → use MUSIC (out of scope, documented) |
| 3 | **RL researcher** | Gym-like `open`/`step`/`run` over the wire, record/stimulus specs | per-session capability *negotiation* endpoint → partial (`/api/neurocontrol/info` lists backends/messages; richer handshake is roadmap) |
| 4 | **Data scientist / analyst** | read-only observer tap, PyO3 module, `(V,L,D,A)` mapping | exact stream alignment → **added** (`ObservationFrame.seq`; observer joins D on `seq`) |
| 5 | **TypeScript / frontend dev** | wire-correct types conforming to `proto/ncp.proto` (today via ts-rs from `ncp-core`; buf/ts-proto is the migration target) | browser transport → WS/Tauri (Zenoh is native, no WASM transport — the typed contract is the unification; documented) |
| 6 | **C++ / systems dev** | native integration | a C/C++ binding → **added** (`ncp-cpp` C ABI + `ncp.h`, compile-and-run verified) |
| 7 | **Embedded / MCU dev** | the JSON wire is compact (binary BulkBlock for bulk data) | `no_std` core + a tiny transport (zenoh-pico / micro-ROS) → **gap / roadmap** (today `ncp-core` is `std`+serde; Zenoh is heavy) |
| 8 | **Security engineer** | per-plane keys, scoped read taps; a default-deny per-plane Zenoh ACL template (`deploy/zenoh-access-control.json5`) + mutual-TLS enablement steps (`SECURITY.md`) | **enable the ACL + mTLS** → on an open realm the command key is world-writable until you do; the template ships the mechanism, live mTLS-enforcement validation is the remaining P0 (#7) |
| 9 | **DevOps / SRE** | reproducible builds, one conformance command | a repeatable check → **added** (`scripts/check.sh`); CI workflow + multi-impl conformance → roadmap |
| 10 | **OSS maintainer / standards** | versioned spec + `.proto` + schemas + extractable crate + semver guard + rationale | neutral spec repo, conformance program, multiple independent impls → **gap / roadmap** (the "become a standard" path) |

Honest summary: lenses 1, 2, 4, 6, 9 were improved this round; 3 and 5 are partial
by design; **8 (auth) is the one real blocker for an open-network deployment** and
needs a security policy; 7 and 10 are roadmap. Use NCP when you need a NEST sim
served live to remote, multi-language clients with safety/provenance and a free
tap — and read `RATIONALE.md` for when a simpler composition (`rmw_zenoh` +
messages + a watchdog) is the better call instead.

## Before you deploy: read the limitations

NCP is pre-1.0 and the contract is deliberately conservative. Before any field or
open-network deployment, read [`KNOWN_LIMITATIONS.md`](KNOWN_LIMITATIONS.md) — an
adversarial audit catalogs **35 findings (3 high, none yet fixed)**: a `BulkBlock`
decode that overlapping columns can drive to OOM, a fail-**open** `ttl_ms` watchdog when
`ttl_ms` is unbounded / non-finite, and a geofence an empty position channel can bypass.
Pair it with [`SECURITY.md`](SECURITY.md): on an open realm the action key is
world-writable until you enable the shipped per-plane ACL + mTLS (lens 8 above).

