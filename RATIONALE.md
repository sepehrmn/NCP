# Why NCP Was Needed — Design Rationale for the Neuro-Cybernetic Protocol

> An unbiased design-rationale document, researched against the robotics-middleware,
> agent-protocol, RL/simulation, and neuroscience-co-simulation ecosystems, and
> deliberately adversarially reviewed for pro-NCP bias. It credits substrate wins
> to the substrate, concedes the strongest "you didn't need a new protocol"
> argument in full, and states NCP's disadvantages as plainly as its advantages.

## Thesis (and an honest caveat about it)

NCP occupies a specific design point: a **versioned, transport-agnostic,
project-agnostic wire contract that serves a running NEST network of point and
rate neurons (spiking, binary, and rate-based models) to external,
possibly-remote robot/UAV/analysis clients** — with neural
record/stimulus semantics, a scientific-provenance boundary on every frame,
QoS-differentiated planes, a first-class command-mode safety concept, and a
read-only observer tap.

The honest caveat, stated up front: "no off-the-shelf protocol occupies this exact
point" is **weak evidence of necessity**. Any sufficiently conjunctive target
(all of A∧B∧C∧D∧E) is unoccupied *by construction* — that is gerrymandering, not
proof. The real test is not "does ROS 2 / MUSIC / MCP *alone* substitute?" (NCP
trivially wins those), but "is the **composition alternative** — `rmw_zenoh` +
three message packages + a watchdog node — meaningfully worse than a bespoke
protocol?" That is the question this document must actually win, and it is treated
in its own section below rather than assumed away.

## What NCP is

NCP is a thin domain-and-provenance layer over a mature transport. The normative
contract is `ncp.proto` plus JSON-Schema mirrors; the reference implementation is
a Rust SDK (`ncp-core` = wire types + safety governor + codec + key scheme;
`ncp-zenoh` = transport; `ncp-gateway` = Engram's Rust edge over a Python NEST
`SessionService`; `ncp-python` = a PyO3 binding; `ncp-core --features ts` =
ts-rs-generated TypeScript types). It defines four keys over a Zenoh realm: a
request/reply **control plane** (`/rpc`), a Best-Effort/conflating **perception
plane** (`…/sensor`), an express, RealTime-priority, safety-gated **action plane**
(`…/command`, with `mode ∈ {init,active,hold,estop}` and a `ttl_ms`), and a
read-only **observation plane** (`…/observation`). The spiking net can act as
perception, action, both, or neither. Every observation carries
`is_simulation_output=true` and `calibrated_posterior=false`: a neuro-controller
is a control artifact, never a paper-reproduction claim.

## Why existing solutions were insufficient

Almost none of the surveyed technologies are alternatives *at the same layer*.
They are transports, serialization formats, or general frameworks that sit
*underneath* an NCP-shaped contract. The verdict throughout: the substrates are
more mature, several are better-pedigreed, but **none provides the
neural/provenance/safety/observer semantics** — and most of NCP's *performance and
topology* advantages are inherited from those substrates, not invented by NCP.

**ROS 2 / DDS.** ROS 2's topics/services/actions map almost 1:1 onto NCP's needs,
and typical robot/UAV clients already live in ROS 2 — it is unambiguously more mature and
better-tooled (rviz, rosbag2, lifecycle nodes). DDS's QoS model is arguably a
*better-specified* version of NCP's three-plane idea with a ~15-year
safety-critical pedigree and µs shared-memory latency. Crucially, DDS already
carries most of NCP's "safety" primitives at the QoS layer: **LIFESPAN** is
*exactly* an auto-expiring `ttl_ms`; **OWNERSHIP/OWNERSHIP_STRENGTH** is exclusive
writer authority over an actuator; **LIVELINESS** is failure-detection → fail-safe;
**DEADLINE** bounds staleness. So `ttl_ms` is **not novel — it is DDS LIFESPAN**.
The genuine NCP-specific safety part is only the `mode` enum (init/active/hold/
estop) as a *wire concept*. The gap that remains: ROS 2 gives the IDL machinery
but no neural V_m/spikes/stimulus vocabulary and no provenance boundary, and
binding the NEST server to the ROS graph/build system burdens non-ROS consumers
(an analysis/observer client may be a plain Rust analyzer, not a ROS node). ROS 2 is the right *client
framework*, not a replacement for the semantic + provenance layer.

**Zenoh alone.** Zenoh is the best substrate match and NCP's chosen transport:
queryables = control-plane RPC, conflation/DROP = perception, express + priority =
action, routed subscriptions = observers, Rust-first core, simpler discovery than
DDS. But it is payload-agnostic *by design* — no typing, no neural semantics, no
provenance. **Every latency, conflation, priority, P2P-no-broker, and
fleet/many-to-many property NCP enjoys is Zenoh's, not NCP's** (see the lens
disclaimer below). Zenoh is the layer NCP sits on, not a competitor.

**MAVLink / MAVROS.** MAVLink owns the UAV actuation edge NCP must *target*
(`CommandFrame` → `SET_POSITION_TARGET` into PX4/ArduPilot via MAVROS), and its
lossy-link tolerance far exceeds anything NCP specifies. But its vocabulary is
fixed and UAV-centric: no neural record/stimulus, no provenance. A downstream
dialect NCP feeds, not a neural-controller protocol.

**gRPC / protobuf.** NCP already uses protobuf IDL, so a gRPC control plane is
idiomatic with broader polyglot tooling than Zenoh queryables. gRPC *does* have
bidirectional streaming; its real limit is **no broker-mediated fan-out, no
late-joining subscriber, no conflation, no topic discovery** — you would hand-roll
a pub/sub broker for the data planes — and HTTP/2/TCP is unfit for the hot action
loop. gRPC could replace NCP's *control plane*; you would still need a data-plane
bus and the neural/provenance layer.

**dm_env_rpc / MCP / ACP / A2A.** The most conspicuous near-peer is **dm_env_rpc**
(DeepMind) — a gRPC/protobuf protocol for serving RL environments across the wire,
which *is* a networked, multi-language env-control protocol very close in spirit to
NCP's control plane; NCP's delta over it is the neural record/stimulus vocabulary,
the QoS-split data planes, the safety mode, and the provenance flag, not the basic
idea of "open/step a remote environment." **MCP** (with the 2025 *streamable HTTP*
transport, not only the older HTTP+SSE) and **ACP/A2A** are request/response
orchestration for LLM tool-use and agent hand-off; even with streamable HTTP their
orchestration model and tool-call latency budget are unfit for a perpetual
sub-10 ms data plane, and they carry no safety authority, no continuous sensor
channel, and no neural vocabulary. They are more mature for *their* layer and
structurally the wrong shape for real-time control — but NCP is right to *resemble*
MCP's versioned capability handshake on its control plane.

**Gym / dm_env / Gymnasium / PettingZoo.** The closest *API-ergonomics* analogue:
`reset`/`step` ↔ NCP `open`/`step`/`run`, and `observation_spec`/`action_spec` is a
clean typed-capability idiom. Gymnasium is the de-facto RL standard with a vastly
larger ecosystem. The *standard specs* are in-process single-language Python (which
is why Rust clients can't call them across the wire) — though note
this is a property of the spec, not a hard law (dm_env_rpc and third-party gRPC
bridges cross the wire). The right ergonomic north star to imitate; not the wire.

**MUSIC and the Neurorobotics Platform / PyNN / NEST Server (the closest prior
art, and the fairest delta).** This neuroscience ecosystem is where NCP must
concede the most. **MUSIC** (Djurfeldt et al. 2010) is the INCF standard for
runtime spike/continuous exchange between simulators via **continuous ports and
event ports wired by connectors** — direct intellectual lineage for NCP's
continuous-(V_m/rate) vs event-(spikes) channel taxonomy, which NCP presents as
*inspired by MUSIC*, not invented. The **ROS-MUSIC toolchain** (Weidel, Djurfeldt,
Duarte, Morrison 2016) did NCP's headline thing in 2016: a real-time NEST→Gazebo
closed loop with sensory→spike encoders and spike→motor decoders, even splitting
transport (MUSIC on the neural side, ROS pub/sub to the robot). **NRP-core** (HBP)
generalizes this with "engines" exchanging **DataPacks (JSON-or-protobuf)** — the
closest *data-model* prior art to NCP's "protobuf IDL + JSON Schemas; the session
declares what to record/inject." A reasonable reviewer asks: *isn't NCP redundant?*

The honest delta is **deployment model and packaging**, not the core idea — and
the wording matters: MUSIC is **co-scheduled in one MPI launch with no dynamic
discovery, no TLS/auth, and no per-client versioned wire contract** (MPI itself
*can* span hosts over TCP/IP or InfiniBand, so the accurate criticism is "one
co-launched MPI world / static config," **not** "same machine / no network"). The
ROS-MUSIC toolchain requires all components co-launched with manual MUSIC config
files, was tested single-node, and explicitly models **no actuator semantics, no
safety constraints, no QoS, no fault tolerance**; its latencies (≈70 ms at a 1 ms
tick to ≈350 ms at a 50 ms tick) are a function of MUSIC tick granularity.
NRP-core is **synchronous, step-locked, blocking, single-host orchestration**
inside a monolithic HBP-coupled platform — not a thin versioned wire you embed in
an arbitrary client. **PyNN/NESTML** describe *what a network is*, not how a robot
talks to a running one — orthogonal. **NEST Server (REST)** is the closest in-tree
"NEST as a network service," and it is request/response only — no streaming planes,
no QoS, no safety. So NCP is **adjacent to and informed by** this work, not
redundant — *provided it cites Weidel et al. 2016, Djurfeldt et al. 2010, and
NRP-core and frames itself as a networked, safety-gated protocolization, never as
the first SNN-robot loop or the inventor of port-typed neural channels.*

## The strongest counter-argument: "compose, don't invent"

The one argument that genuinely threatens NCP's existence, stated in full so it
must be answered:

> *Take `rmw_zenoh` (ROS 2 on a Zenoh middleware, Tier-1 since 2025). Define three
> message packages — `neuro_msgs/RecordFrame`, `StimulusFrame`, `CommandFrame` —
> each with `bool is_simulation_output`, `bool calibrated_posterior`, and a
> `SimProvenance` sub-message. Set per-topic QoS: Reliable + Deadline + Lifespan
> for action, Best-Effort + KeepLast(1) for perception. Implement ESTOP/HOLD/TTL
> as a node-level lifecycle plus a Deadline/Liveliness/Lifespan watchdog. Non-ROS
> clients (analysis/observer clients, robot/UAV bodies) read the same Zenoh keys via `zenoh-rs` directly. You
> now have neural semantics, provenance, QoS planes, a free observer, and fleet —
> with zero new protocol, zero new SDK, and ROS 2's entire tooling and governance
> for free. What does `ncp.proto` + `ncp-core` + `ncp-zenoh` + PyO3 buy that this
> does not?*

This is strong and **partially correct**. Honest answers, with their weaknesses
conceded:

1. **Off-ROS reach.** ROS 2 message payloads are CDR with `.msg` type hashes;
   consuming them off-ROS means a CDR+type-hash story, whereas bare protobuf/JSON
   is more directly polyglot (PyO3/ts-rs/any-proto). *Weakness:* `rmw_zenoh`
   clients **can** speak at the Zenoh layer, so this is a convenience delta, not a
   wall.
2. **Safety as authority vs. as QoS.** NCP makes `mode` (estop/hold) an explicit
   wire state a controller asserts; the composition makes safety an emergent
   property of Deadline/Liveliness/Lifespan watchdogs. *Weakness:* this is a
   **design opinion, not a fact** — a hardened DDS Deadline/Ownership/Liveliness
   stack is arguably *safer* than a v0.1 single-author `mode` enum (see Lens 6).
3. **One audited safety/codec implementation** in `ncp-core`, reached by all
   languages via FFI, beats three reimplementations of ESTOP/HOLD/TTL/encoding.
   *Weakness:* only true once that one crate is actually audited and conformance-
   tested — which it is not yet.
4. **No ROS build/graph imposed** on a plain analyzer like an analysis/observer client. *Weakness:*
   real but modest; the cost is a dependency, not impossibility.

Net: the composition alternative is **viable and cheaper to own**, and a team
already standardized on ROS 2 should probably take it. NCP's defensible edge is for
the **non-ROS, multi-language, fleet+observer** case where carrying the whole ROS
2 build system to every consumer is the larger tax — and even there the win is
*packaging and polyglot ergonomics*, not a capability ROS 2 + Zenoh lacks.

## The ten lenses

> **Disclaimer (read first).** Where a listed advantage is the *substrate's*
> (Zenoh pub/sub, conflation, express/priority, P2P/no-broker, datacenter-to-LPWAN
> span, DDS/ROS2 bridging, multi-subscriber "free taps", fleet many-to-many), it
> is **credited to Zenoh/protobuf, not to NCP**. Only the neural record/stimulus
> vocabulary, the provenance discriminators, the `mode` safety enum, and the typed
> `(V,L,D,A)` observer semantics are NCP's own. Lenses 2–4 and 8 in particular
> describe inherited properties.

**1. Scientific provenance & boundary.** *Advantage:* every observation carries
`is_simulation_output=true`/`calibrated_posterior=false` as cheap, mandatory,
fail-closed fields — a machine-checkable epistemic discriminator on the hot path.
*Disadvantage:* no upstream standard defines this term — W3C PROV models lineage
but carries no epistemic-status field, RO-Crate packages archives, nanopublications
are publishing-weight; NCP's flag is a domain assertion on no incumbent, and the
PROV-aligned session archive is a recommendation, not shipped.

**2. Latency & performance.** *Advantage (substrate):* Zenoh gives µs–low-ms with
express, priorities, and conflation; the plane split lets each pay only its cost.
*Disadvantage:* software NEST over Zenoh will not approach neuromorphic on-chip
loops (figures like "~3 ms on Loihi, faster than real-time" are **task- and
benchmark-specific demos, not a universal property**); NCP must not claim latency
leadership, and the gateway adds a localhost-socket hop to Python NEST.

**3. Coupling & topology / fleet.** *Advantage (substrate):* data-centric pub/sub
lets a fleet and observers attach without re-plumbing; Zenoh is P2P-capable.
*Disadvantage:* **fleet is a solved, deployed ROS 2/DDS pattern** (namespacing,
domain IDs, discovery); NCP inherits it via Zenoh and neither adds nor improves on
it — and inherits its **unsolved multi-writer / who-steps-when coordination**
(cf. PettingZoo AEC-vs-Parallel), which NCP exposes but does not resolve.

**4. Transport-agnosticism & medium choice.** *Advantage:* the wire is
JSON/protobuf, identical across transports. *Disadvantage:* agnosticism is partly
aspirational — the only shipped transport is `ncp-zenoh`; Zenoh is native and does
not compile to browser WASM (TS gets *types* via ts-rs, but the browser transport
is BYO WebSocket); DDS's safety-pedigreed QoS is an unexercised alternative.

**5. Language & runtime interop (Rust ↔ Python ↔ TS ↔ C++).** *Advantage:* two
complementary sources of truth — `ncp.proto` owns the *wire* (any peer
interoperates with zero Rust), `ncp-core` owns the *behavior* (codec, safety,
keys, version), reaching Python via PyO3 and C/C++ via a C ABI (`ncp-cpp` +
`ncp.h`) — both verified by importing/linking and running — so the
high-consequence safety/codec logic is written once, not reimplemented per
language. *Disadvantage:* FFI glue is irreducible; PyO3/C-ABI friction with Rust
enums/generics constrains the public surface to FFI-friendly tagged structs and
JSON-string in/out; TS relies on *generated types + a hand-kept transport* (Zenoh
is native, no browser WASM), so TS is the least-unified peer.

**6. Safety & control authority.** *Advantage:* the action plane has an explicit
`mode` (init/active/hold/estop) the controller asserts on the wire, with the
robot/UAV mapping failing safe to zero velocity on hold/estop — a *protocol* concept
no surveyed alternative states as an enum (though DDS LIFESPAN≈`ttl_ms` and
OWNERSHIP/LIVELINESS cover much of the rest). *Disadvantage (confronted directly):*
this is a **v0.1, single reference implementation, no conformance suite**, on a bus
that **has no authentication yet** — i.e. anyone who can reach the bus can publish
to `…/command`. A bespoke, unaudited ESTOP path on an unauthenticated bus is
plausibly *less* safe than a 15-year-pedigreed DDS Deadline/Liveliness/Ownership
stack, not more. The "first-class safety governor" claim and the "no auth on the
open bus" admission are in genuine tension until auth + a conformance suite ship.

**7. Domain semantics.** *Advantage:* a **networked, versioned, transport-agnostic
wire vocabulary** for record (V_m/spikes/rate) and stimulus (current_pA/rate_hz/
spike_times/weight_set) against named populations. *Disadvantage:* the *underlying
record/stimulus modeling is MUSIC/NRP prior art*, not novel; it is NEST-shaped
today, and the "simulator-agnostic" ambition is untested *as shipped* — though it
is agnostic *by design*: the typed enums are abstract SNN concepts each backend
maps, with simulator-specifics confined to the backend + the `recordables`/`params`
escape hatches, at **zero NEST cost** (see `ROADMAP.md` "Future direction:
simulator-agnosticism"). A second backend is the test. (So: NCP-only as a *wire
vocabulary*, not as a concept.)

**8. Observability & analysis.** *Advantage:* an analysis/observer client's `ncp-observer` subscribes
read-only and turns each tick into a typed `(V,L,D,A)` PID sample. *Disadvantage:*
**the "free tap" is a property of the bus, not of NCP** — any DDS/ROS 2 topic is
multi-subscriber (`ros2 bag record`, `rostopic echo`, DDS spy are the same free
tap); NCP's only contribution here is the typed `(V,L,D,A)`+provenance *semantics*
on the frame. And the same free-read property means the action plane is
**world-writable on an unauthenticated bus** — a safety *and* security hole, not
only a feature. Alignment uses `seq`: `SensorFrame`, `CommandFrame`,
`ControlStatus`, and `ObservationFrame` all carry a `seq` field (see
`messages.rs`, `schemas/observation_frame.schema.json`, the `.proto`, and the TS
binding), so a split-plane observer can join `(V,L,D,A)` on `seq` rather than on
arrival time. In the pure pull/sim-service path (no controller) `ObservationFrame.seq`
is `0`; inside a closed loop it echoes the driving `SensorFrame.seq`.

**9. Ecosystem maturity, adoption & risk.** *Advantage:* rides proven substrates
rather than reinventing them. *Disadvantage (the NIH critique, and the cost of
ownership):* NCP is a single-author reference SDK; ROS 2, DDS, Gazebo, MCP, A2A,
and Gymnasium are vastly more adopted and governed. NCP visibly **reinvents parts
of MUSIC (ports), ROS/DDS (QoS planes, LIFESPAN), and MCP (handshake/schemas)**.
Beyond adoption, **build-vs-buy cost** is the con a principal engineer weighs most:
a bespoke Rust+PyO3+ts-rs+protobuf+Zenoh stack is a perpetual maintenance liability
(FFI churn, Zenoh API breakage, version-compat upkeep, a conformance suite that
does not yet exist) versus inheriting ROS 2/DDS's maintained tooling.

**10. Developer experience, governance & standardization path.** *Advantage:*
schema-first contract, an extractable crate (no Paper2Brain dependency), a
Gym-familiar ergonomic target. *Disadvantage:* "become a standard like MCP" is a
multi-year governance effort needing an open spec repo, a conformance suite,
multiple independent implementations, and a neutral home (the LF AI & Data path
ACP/A2A took) — none of which exists yet.

## Disadvantages & open risks (summary)

NCP is **v0.1 with a single reference implementation and no conformance suite**
(wire-compat tests are not a multi-implementor program). The **Python NEST server
is still the real brain**; the Rust gateway is a localhost-socket bridge, so NCP
does not yet own the hot integrator path. There is **no auth on the open bus** —
the action plane is world-writable to anyone who can reach it. It **reinvents
parts of MUSIC, ROS/DDS (incl. LIFESPAN≈ttl), and MCP**, and must not claim to have
invented SNN-robot loops, port-typed neural channels, protobuf neural datapacks, or
latency leadership. Transport-agnosticism is Zenoh-only today; the TS peer is the
least-unified; observer D-alignment is best-effort; the PROV/RO-Crate archive is
unshipped; and the **composition alternative (`rmw_zenoh` + `neuro_msgs` + a
watchdog) is a real, cheaper-to-own option** that a ROS-2-standardized team should
prefer.

## What NCP deliberately borrows

- **Zenoh** — queryables (RPC), conflation/DROP (perception), express/RealTime
  (action), routed subscriptions (observers). *All inherited, not invented.*
- **MCP-style versioned schemas + capability handshake** — `ncp_version`
  negotiation and "learn what a backend supports, fail-closed on the unsupported."
- **MUSIC's port/connector taxonomy** — continuous-(V_m/rate) vs event-(spikes)
  channels, acknowledged as MUSIC lineage.
- **ROS/DDS QoS thinking** — the per-plane reliability/priority/conflation split,
  and `ttl_ms` ≡ DDS LIFESPAN. Validation, not novelty.
- **Gym/dm_env(_rpc) ergonomics** — `open`/`step`/`run` and `*_spec`-style typed
  capabilities.
- **PROV / RO-Crate** — the intended provenance substrate for the session archive.

## When you should NOT use NCP / use X instead

- **Coupling two simulators on one HPC cluster:** **MUSIC**. NCP doesn't replace it.
- **An all-ROS 2 stack, especially with no off-ROS consumers:** define `neuro_msgs`
  on **`rmw_zenoh`** with a watchdog (the composition above) and keep ROS 2's
  tooling/governance — this is the right default for a ROS-standardized team.
- **A safety-critical path needing a decade of QoS hardening:** **DDS** as the
  substrate (heavier, fiddlier, pedigreed) — possibly *under* an NCP contract.
- **In-process, single-language Python RL:** **Gymnasium/dm_env** (or **dm_env_rpc**
  if you need it networked) — no bespoke wire required.
- **LLM tool-use / agent hand-off:** **MCP / A2A**.
- **Driving a flight controller directly:** **MAVLink/MAVROS** is the edge NCP
  *targets*, not replaces.
- **Raw latency near silicon:** neuromorphic on-chip (**Loihi/SpiNNaker**).

Use NCP when you need *all* of: a NEST SNN served to external/remote multi-language
non-ROS clients, neural record/stimulus semantics, a safety-moded action plane, a
provenance boundary on every frame, and a typed analysis tap — **and** carrying the
full ROS 2 build system to every consumer is the larger tax. That intersection,
weighed honestly against the composition alternative, is the narrow gap NCP fills.
