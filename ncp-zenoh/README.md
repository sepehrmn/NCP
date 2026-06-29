# ncp-zenoh

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The recommended **decoupled NCP transport**: carries the Neuro-Cybernetic Protocol — as opaque byte payloads it never decodes — over a
data-centric [Zenoh](https://zenoh.io) bus. RPC is a *queryable* on `{realm}/rpc`; the perception,
action and observation **data planes** are *pub/sub* on per-session keys. Peers address data, not
server addresses — location-transparent, many-to-many, and the medium a ROS 2 robot client already
speaks.

This is the Rust transport binding in the polyglot NCP SDK. NCP defines **one normative wire
protocol** ([`NEURO_CYBERNETIC_PROTOCOL.md`](../NEURO_CYBERNETIC_PROTOCOL.md)) with peers in Rust,
Python, TypeScript and C++; `ncp-zenoh` wraps a `zenoh::Session` (`ZenohBus`) with the NCP key
scheme and per-plane QoS, and provides a typed client (`ZenohNcpClient`) plus a streaming
`ZenohControlTransport` for the closed control loop.

Each plane gets the QoS its job needs (`Plane`): **perception** drops under congestion (DataHigh),
**action** is express + drop at RealTime priority (lowest-latency setpoint), and
**control/observation** does not drop under congestion (BLOCK — it back-pressures the publisher rather than dropping). Note this is *congestion control*, not the wire-level retransmit/reliability API: NCP sets congestion control + priority + express only, leaving wire reliability at Zenoh's default (the minimal feature set does not enable the `unstable` reliability API — see lib.rs:11-20). See [Per-plane QoS, from first principles](#per-plane-qos-from-first-principles) below.

## Open a bus and run a typed client

```rust,ignore
use ncp_zenoh::{ZenohBus, ZenohNcpClient};

// open() uses the default Zenoh config + default realm; open_realm(keys) sets the realm.
let bus = ZenohBus::open().await?;
let client = ZenohNcpClient::new(bus);
let opened = client.open(&open_session_msg).await?; // version-checked SessionOpened
```

For a secured deployment, build an explicit `zenoh::Config` (re-exported as `ZenohConfig`) with TLS
+ access control and pass it to `ZenohBus::with_config(config, keys)`; see
[`SECURITY.md`](../SECURITY.md) for enabling mutual TLS and per-plane ACLs.

See the [repository README](../README.md) for the full SDK overview, and
[`NEURO_CYBERNETIC_PROTOCOL.md`](../NEURO_CYBERNETIC_PROTOCOL.md) for the normative spec.
## Per-plane QoS, from first principles

The three planes carry traffic with fundamentally different time-and-consistency
requirements, so a single global QoS would be wrong for at least two of them. NCP
matches each plane's Zenoh QoS to the *control-theoretic role* of its stream
(`Plane` in [`src/lib.rs`](src/lib.rs)):

- **Perception (sensor → controller): freshness beats completeness.** A closed
  loop acts on the *latest* world state; a stale sensor frame is worse than a
  dropped one, because it injects delay into the feedback path (eroding phase
  margin, and at the limit destabilising the loop). So the plane uses
  `CongestionControl::Drop` (lib.rs:110): when the TX queue is full, drop rather
  than block the publisher or grow latency. `Priority::DataHigh` keeps fresh
  perception ahead of bulk/observation traffic. Caveat from the code (lib.rs:15-17):
  this DROP is **not conflation** — under congestion it drops *some* frames, not
  necessarily down to the newest, and there is no last-value guarantee at this
  layer.
- **Action (controller → actuator): minimum latency, safety-gated by the sender.**
  The setpoint is the most latency-critical message in the loop — every
  microsecond of actuation delay is dead time. So the plane sets `express=true`
  (lib.rs:121-124) to kill batching (send immediately, don't wait to coalesce),
  `Priority::RealTime` to jump every queue, and `CongestionControl::Drop` so a
  backed-up link never blocks the controller. Reliability is intentionally *not*
  bought here: a re-sent stale setpoint is useless because the next tick
  supersedes it (latest-wins). Safety is enforced by the **sender**
  (`SafetyGovernor` in `ncp-core`) *before* the publish, not by the transport.
- **Control / observation: correctness beats latency.** Lifecycle RPC replies
  (session open/close/step) and observation/analysis broadcasts must arrive intact
  and in order — a dropped `SessionClosed` or a half-delivered observation
  corrupts state or analysis. So the plane uses `CongestionControl::Block`
  (lib.rs:111): under congestion the publisher back-pressures rather than dropping.
  Observation reuses the Control profile, so **keep the observation stream
  low-rate** or it will back-pressure the publisher (lib.rs:22-24).

Why not "just make everything reliable"? Reliability on the perception/action
planes would convert *loss* into *delay* — exactly the wrong trade for a feedback
loop, where bounded staleness is tolerable but unbounded latency is not. The split
lets each plane fail in the way its consumer can absorb: data planes shed load,
the control plane back-pressures.

## Encoding-agnostic: the transport carries raw bytes

`ncp-zenoh` is a *byte pipe with QoS*. Every data-plane method is byte-oriented:
`put` / `put_sensor` / `publish_command` / `publish_observation` take `&[u8]`,
`request` returns `Vec<u8>`, and the `serve_rpc` / `subscribe` callbacks receive
`Vec<u8>`. The transport never parses, validates, or re-encodes a payload — it
routes opaque bytes with the plane's QoS. (The typed convenience layer,
`ZenohNcpClient` / `ZenohControlTransport`, does `serde_json` *on top* of this byte
transport; that is a convenience built over the pipe, not the pipe itself.)

What actually rides those bytes today (the shipped runtime reality):

- **Sensor / Command / RPC planes — JSON (`serde_json`).** Human-readable and
  debuggable; this is the default. `ZenohControlTransport` serialises
  `CommandFrame` and deserialises `SensorFrame` with `serde_json` (lib.rs:509, 531).
- **Bulk / observation (analysis) data — the binary `BulkBlock`** columnar format
  from `ncp-core` (`bulk.rs`), for large arrays where JSON's size and parse cost
  would dominate.

Note that **protobuf is the schema contract, not the runtime encoding.**
`proto/ncp.proto` (+ `gen/`) is the normative IDL and the conformance
source-of-truth, but the generated Rust bindings are not compiled into any runtime
path (`gen/rust` is not a workspace member; there is no `prost` dependency). So
when the SDK says "the wire is JSON/protobuf", read it as *protobuf defines the
schema that the JSON conforms to* — JSON (and binary `BulkBlock` for bulk) is what
actually ships.

Because the transport is encoding-agnostic, a bandwidth- or kHz-constrained
consumer could negotiate a denser on-the-wire encoding (e.g. protobuf bytes) as an
opt-in *without changing `ncp-zenoh`* — the bytes are the bytes. JSON stays the
debuggable default.

> Security note: `BulkBlock::decode` has an audited unbounded-allocation finding
> (overlapping/duplicate columns enable a large memory amplification → OOM DoS).
> Validate bulk payloads from untrusted peers accordingly — see the High entry
> (`bulk.rs`) in [`KNOWN_LIMITATIONS.md`](../KNOWN_LIMITATIONS.md). It is *not*
> fixed.

## Zero-copy and per-frame copies (overhead)

`ncp-zenoh` enables Zenoh's `shared-memory` feature
([`Cargo.toml`](Cargo.toml)) so same-host peers (e.g. an in-sim plant and the
controller process) can move frames via SHM **zero-copy** instead of a socket
round-trip.

That zero-copy is, however, currently undercut on the publish path:
`ZenohBus::put` calls `payload.to_vec()` on **every** publish (lib.rs:441),
copying the whole frame even when the caller already owns a freshly-serialised
`Vec<u8>`. Moving the owned buffer into Zenoh `ZBytes` instead (e.g. a `put_owned`
or an `impl Into<ZBytes>` overload) would remove one alloc+copy per publish **with
no wire change** — the single highest-value *safe* optimisation for this crate.
The mirror cost on the receive side: the `subscribe` callback allocates a fresh
`String` + `Vec<u8>` per frame (lib.rs:468). Both are catalogued (with the
`send_command` per-tick allocations) in
[`KNOWN_LIMITATIONS.md`](../KNOWN_LIMITATIONS.md) (`lib.rs:441`, `lib.rs:468`,
`lib.rs:530`); none are applied yet.

Scale matters here: end-to-end NCP control-tick overhead is on the order of **~1
µs** (frame ser/de plus the safety gates; see [`PERFORMANCE.md`](../PERFORMANCE.md)
and [`ncp-core/examples/overhead.rs`](../ncp-core/examples/overhead.rs)), i.e.
~0.003-0.1% of a 20-1000 Hz control budget — the in-sim / NEST compute dominates.
These copies are a constant-factor cleanup on an already-cheap path, not a
correctness issue.

## License

Licensed under either of [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE) at your option.
