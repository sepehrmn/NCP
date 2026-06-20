# ncp-zenoh

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The recommended **decoupled NCP transport**: carries the Neuro-Cybernetic Protocol over a
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
**control/observation** is reliable (BLOCK, no drop).

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

## License

Licensed under either of [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE) at your option.
