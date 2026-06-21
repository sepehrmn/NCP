# ncp (Python) — PyO3 bindings for the NCP Rust core

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

PyO3 bindings (the importable `ncp` module) for the [`ncp-core`](../ncp-core) Rust
reference implementation of the Neuro-Cybernetic Protocol.

In the polyglot NCP SDK there is **one normative protocol** with peers in Rust,
Python, TypeScript, and C++. This crate is the **Python peer**: so Python projects
use the canonical Rust implementation rather than reimplementing the wire, the
version guard, the key scheme, the rate codec, the action-plane safety governor, and
message validation all come from `ncp-core`. Any Python peer can compute keys,
encode/decode, and validate frames through this module and be guaranteed
wire-identical to the Rust and TS peers.

This is a [maturin](https://github.com/PyO3/maturin) extension module, built as an
`abi3` wheel — not via plain `cargo`. The `extension-module` feature is **off by
default** so `cargo build`/`check`/`test --workspace` works on Linux/Windows;
maturin enables it explicitly.

## Build

```bash
maturin develop -m ncp-python/Cargo.toml --features extension-module
```

## Use

```python
import ncp

ncp.NCP_VERSION                      # "0.5"
k = ncp.Keys("ncp")                  # the realm is a deployment choice (e.g. "engram/ncp")
k.command("uav3")                    # "ncp/session/uav3/command"
ncp.decode_command(codec_json, '{"vel_x":200.0}', t=0.0, seq=7)  # CommandFrame JSON
```

The module also exposes `check_version`, `encode_rates`, `govern` (the safety
governor), `validate` (kind-aware wire validation), and `channel_value`.

See the normative spec [`NEURO_CYBERNETIC_PROTOCOL.md`](../NEURO_CYBERNETIC_PROTOCOL.md)
and the [repository README](../README.md) for the full protocol, the message kinds,
and the other language peers.

## License

Licensed under either of [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE) at your option.
