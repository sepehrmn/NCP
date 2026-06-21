# e2e — live cross-process / cross-language NCP integration

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The conformance corpus ([`../conformance/`](../conformance)) pins the wire **shape** and
the per-peer **decisions**. This directory pins the thing those can't: that the contract
actually **flows end-to-end across a real process + language boundary, over a real
transport** — the property a release has to guarantee.

The key idea is that the **backend and the contract are separable**. NEST advances a
kernel; the *wire* doesn't care whether the frames came from NEST or from engram's
NEST-free `MockBackend` — both emit real `Observation` frames. So the full medium +
contract is testable **without NEST and without `zenoh-python`**, in any sandbox.

## What's covered (and where it gates)

| Tier | What it proves | Where | Gates in CI? |
|---|---|---|---|
| **Cross-process, production transport** | Two **independent Zenoh sessions** over a real tcp link drive the full `open→step→run→close` RPC through the typed `ZenohNcpClient` (incl. the version + advisory-contract handshake), boundary intact | [`../ncp-zenoh/tests/cross_session_rpc.rs`](../ncp-zenoh/tests/cross_session_rpc.rs) | ✅ `cargo test` |
| **Cross-process, real server** | engram's **real** `SessionService` (over a localhost-TCP socket, `MockBackend`) serves the lifecycle across a process boundary; plus **forward/backward compatibility** (unknown future field accepted, omitted optionals defaulted) — the non-breaking-evolution guarantee | `Paper2Brain/backend/neurocontrol/test_e2e_cross_process.py` | ✅ engram smoke job |
| **Cross-language** | a **Rust** client ([`../ncp-core/examples/ncp_tcp_client.rs`](../ncp-core/examples/ncp_tcp_client.rs)) drives the **Python** engram server over the wire (a crebain/pid_vla peer ↔ engram), contract verified | `run_cross_language_e2e.py` (this dir) | local (needs both repos) |
| **Cross-language *decisions*** | all four peers (Rust/Python/C++/TS) decide identically on `check_version`/`contract_status`/`validate`/`govern` | [`../conformance/behavior/`](../conformance/behavior) | ✅ all peers |

Together these exercise the medium (Zenoh **and** localhost-TCP), the contract (handshake,
lifecycle, the mandatory scientific-boundary discriminators), the languages (Rust ↔ Python),
and **future-extensibility without breaking changes** (the additive-field forward-compat
test here + the `buf breaking` wire gate in CI).

## Running it

```text
# Production transport, cross-session Zenoh (self-contained, gates in CI):
cargo test -p ncp-zenoh --test cross_session_rpc

# engram real server, cross-process (NEST-free; gates in engram's smoke job):
#   (in Paper2Brain) python -m pytest backend/neurocontrol/test_e2e_cross_process.py

# Full cross-language picture (needs this repo + a sibling Paper2Brain + cargo + python):
python3 e2e/run_cross_language_e2e.py            # or: --engram /path/to/Paper2Brain
```

The cross-language runner stands up engram's `bridge_server --backend mock` (the Python side
of `ncp-gateway`) and drives it from a Python and a Rust client, asserting each completes the
lifecycle with the contract intact. It **skips with a clear message** if a sibling
`Paper2Brain` checkout isn't found — the two component halves (the Zenoh cross-session test
and the behavioral corpus) still gate on their own.

## License

Licensed under either of [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE) at your option.
