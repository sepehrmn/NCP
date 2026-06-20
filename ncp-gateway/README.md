# ncp-gateway

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Engram's Rust NCP edge: a **binary** that runs the Zenoh bus (queryable RPC + observation
pub/sub) and bridges control-plane RPC to the in-process Python `SessionService`.

Engram's brain is NEST (Python), so its NCP *server* stays Python. This gateway gives Engram a
production-grade Rust Zenoh edge: it runs the control-plane RPC queryable (`{realm}/rpc`) and the
observation pub/sub, then forwards each RPC to the Python `bridge_server.py` over a localhost
socket — reusing the transport-neutral `handle_json` seam. The fleet-facing, latency-sensitive
transport becomes Rust (SHM/QoS, many-to-many discovery, free observer taps); `nest.Run` stays
in Python.

In the polyglot NCP SDK, one normative wire contract is spoken by peers in Rust, Python, TypeScript,
and C/C++. `ncp-gateway` is the Rust deployment edge in front of a Python commander; it builds on
[`ncp-core`](../ncp-core) (keys/realms) and [`ncp-zenoh`](../ncp-zenoh) (the Zenoh transport).

```text
 Zenoh bus  ──(SHM/QoS)──►  ncp-gateway (this)  ──(TCP, newline-JSON)──►  bridge_server.py
    ▲                          {realm}/rpc queryable                      SessionService.handle_json → nest.Run
    └── robot/UAV bodies, analysis/observer clients, dashboards attach as peers / observers
```

## Run

```text
cargo run -p ncp-gateway
```

Configuration is via environment variables:

```text
NCP_REALM        key-expression realm           (default: engram/ncp)
NCP_BRIDGE_ADDR  Python bridge_server.py addr   (default: 127.0.0.1:28474)
```

The gateway serves NCP RPC on `{realm}/rpc` and the observation/sensor/command planes on
`{realm}/session/<id>/<plane>`. Ctrl-C to stop.

## See also

- The normative wire contract: [`NEURO_CYBERNETIC_PROTOCOL.md`](../NEURO_CYBERNETIC_PROTOCOL.md)
- Repository overview: [NCP README](../README.md)

## License

Licensed under either of [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE) at your option.
