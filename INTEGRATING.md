# Integrating NCP into a new project

NCP is designed so adopting it is **simple and minimally invasive** — and, for the
"a NEST point/rate neural network served live to external clients" use case, better than the
alternatives (the honest comparison, including where it is *not*, is in
[`RATIONALE.md`](RATIONALE.md)).

## The minimal-invasiveness contract

1. **Engram stays project-agnostic.** It speaks only NCP (entity/channel-addressed
   messages). It carries **no** project topic names, message types, or field
   layouts. So adding your project changes **zero** Engram code.
2. **Your project owns its mapping, in your repo, in your language.** A thin
   adapter maps your sensors/actuators ↔ NCP `SensorFrame`/`CommandFrame`/stimulus/
   record. That is the *only* code you write.
3. **It bolts on without touching what you have.** A robot/UAV client's
   integration can be a self-contained `src/ncp/` module behind an off-by-default
   `ncp` Cargo feature — the default build and its command-contract test stay
   unchanged. An analysis/observer client's is a read-only observer crate that
   drives nothing. Neither needs to edit existing code.

## 5-minute quickstart, per language (one canonical Rust core)

**Rust** — depend on the SDK; subscribe/publish the planes or open a session:
```rust
let bus = ncp_zenoh::ZenohBus::open().await?;                 // realm engram/ncp
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
char* key = ncp_key_command("engram/ncp", "uav1");   // ncp_string_free(key)
char* cmd = ncp_decode_command(codec_json, rates_json, 0.0, 7,
                               /*frame_id=*/NULL, /*mode=*/NULL);
```

In every language the *behavior* (key scheme, codec, version guard, safety,
validation) comes from the one Rust core, so all peers are wire-identical.

## Picking the integration mechanism (decreasing preference)

1. **Client-side adapter** (best) — your NCP client + mapping in your repo, against
   `ncp-core`/`ncp.proto`/`schemas/` (your client's own `src/ncp/` adapter module).
2. **Declarative profile** (data, not code) — a JSON mapping a generic loader
   consumes; no per-project class in Engram (a declarative profile shipped by your
   client).
3. **Plugin package** — `engram-ncp-<you>` registering a profile via entry points.

## Evaluated from 10 adopter lenses — what each gets, what was missing, what changed

| # | Adopter | Gets | Was missing → status |
|---|---|---|---|
| 1 | **Robotics / UAV engineer** | action + perception planes, pose/velocity↔frame mapping, safety mode | streaming control loop over the bus → **added** (`ZenohControlTransport`) |
| 2 | **Computational neuroscientist** | live data from a *running* NEST kernel (persistent `Prepare`/`Run`), V_m/spikes/rate, stimulus injection | "is it real-time like MUSIC?" → **answered** ([`NEST_REALTIME.md`](NEST_REALTIME.md)); multi-sim coupling → use MUSIC (out of scope, documented) |
| 3 | **RL researcher** | Gym-like `open`/`step`/`run` over the wire, record/stimulus specs | per-session capability *negotiation* endpoint → partial (`/api/neurocontrol/info` lists backends/messages; richer handshake is roadmap) |
| 4 | **Data scientist / analyst** | read-only observer tap, PyO3 module, `(V,L,D,A)` mapping | exact stream alignment → **added** (`ObservationFrame.seq`; observer joins D on `seq`) |
| 5 | **TypeScript / frontend dev** | wire-correct types conforming to `proto/ncp.proto` (today via ts-rs from `ncp-core`; buf/ts-proto is the migration target) | browser transport → WS/Tauri (Zenoh is native, no WASM transport — the typed contract is the unification; documented) |
| 6 | **C++ / systems dev** | native integration | a C/C++ binding → **added** (`ncp-cpp` C ABI + `ncp.h`, compile-and-run verified) |
| 7 | **Embedded / MCU dev** | the JSON/proto wire is small | `no_std` core + a tiny transport (zenoh-pico / micro-ROS) → **gap / roadmap** (today `ncp-core` is `std`+serde; Zenoh is heavy) |
| 8 | **Security engineer** | per-plane keys, scoped read taps | **auth/ACL on the action plane** → **open risk** (the command key is world-writable on an open bus; enable Zenoh access-control/TLS via config; top priority, needs a deployment policy) |
| 9 | **DevOps / SRE** | reproducible builds, one conformance command | a repeatable check → **added** (`scripts/check.sh`); CI workflow + multi-impl conformance → roadmap |
| 10 | **OSS maintainer / standards** | versioned spec + `.proto` + schemas + extractable crate + semver guard + rationale | neutral spec repo, conformance program, multiple independent impls → **gap / roadmap** (the "become a standard" path) |

Honest summary: lenses 1, 2, 4, 6, 9 were improved this round; 3 and 5 are partial
by design; **8 (auth) is the one real blocker for an open-network deployment** and
needs a security policy; 7 and 10 are roadmap. Use NCP when you need a NEST sim
served live to remote, multi-language clients with safety/provenance and a free
tap — and read `RATIONALE.md` for when a simpler composition (`rmw_zenoh` +
messages + a watchdog) is the better call instead.
