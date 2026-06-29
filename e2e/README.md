# e2e — live cross-process / cross-language NCP integration

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The conformance corpus ([`../conformance/`](../conformance)) pins the wire **shape** and
the per-peer **decisions**. This directory pins the thing those can't: that the contract
actually **flows end-to-end across a real process + language boundary, over a real
transport** — the property a release has to guarantee.

The key idea is that the **backend and the contract are separable**. NEST advances a
kernel; the *wire* doesn't care whether the frames came from NEST or from engram's
NEST-free `MockBackend` — both emit real `Observation` frames. So the full medium +
contract is testable **without NEST and without `zenoh-python`**, in any sandbox. That
proves you don't *need* NEST to verify the contract; the **converse** — that the contract
*faithfully carries* a real NEST kernel, across heterogeneous neuron-model families — is
pinned by [`nest_five_networks.py`](nest_five_networks.py) (see below).

## What's covered (and where it gates)

| Tier | What it proves | Where | Gates in CI? |
|---|---|---|---|
| **Cross-process, production transport** | Two **independent Zenoh sessions** over a real tcp link drive the full `open→step→run→close` RPC through the typed `ZenohNcpClient` (incl. the version + advisory-contract handshake), boundary intact | [`../ncp-zenoh/tests/cross_session_rpc.rs`](../ncp-zenoh/tests/cross_session_rpc.rs) | ✅ `cargo test` |
| **Cross-process, real server** | engram's **real** `SessionService` (over a localhost-TCP socket, `MockBackend`) serves the lifecycle across a process boundary; plus **forward/backward compatibility** (unknown future field accepted, omitted optionals defaulted) — the non-breaking-evolution guarantee | `engram/backend/neurocontrol/test_e2e_cross_process.py` | ✅ engram smoke job |
| **Cross-language** | a **Rust** client ([`../ncp-core/examples/ncp_tcp_client.rs`](../ncp-core/examples/ncp_tcp_client.rs)) drives the **Python** engram server over the wire (a crebain/[prisoma](https://github.com/sepahead/prisoma) peer ↔ engram), contract verified | `run_cross_language_e2e.py` (this dir) | local (needs both repos) |
| **Real-simulator breadth** | the **same** RPC contract carries **5 distinct real NEST spiking models** (current-LIF `iaf_psc_alpha`, exp-synapse `iaf_psc_exp`, `izhikevich`, Hodgkin–Huxley `hh_psc_alpha`, adaptive-EIF `aeif_cond_alpha`) — `current_pA` stimulus in, recorded `spikes` out, `is_simulation_output` frames genuinely from a NEST kernel; only `network.ref` changes per model | [`nest_five_networks.py`](nest_five_networks.py) (this dir) | local (needs NEST + engram) |
| **Cross-language *decisions*** | all four peers (Rust/Python/C++/TS) decide identically on `check_version`/`contract_status`/`validate`/`govern` | [`../conformance/behavior/`](../conformance/behavior) | ✅ all peers |

Together these exercise the medium (Zenoh **and** localhost-TCP), the contract (handshake,
lifecycle, the mandatory scientific-boundary discriminators), the languages (Rust ↔ Python), the real-simulator breadth (5 NEST neuron-model families),
and **future-extensibility without breaking changes** (the additive-field forward-compat
test here + the `buf breaking` wire gate in CI).

## Running it

```text
# Production transport, cross-session Zenoh (self-contained, gates in CI):
cargo test -p ncp-zenoh --test cross_session_rpc

# engram real server, cross-process (NEST-free; gates in engram's smoke job):
#   (in engram) python -m pytest backend/neurocontrol/test_e2e_cross_process.py

# Full cross-language picture (needs this repo + a sibling engram + cargo + python):
python3 e2e/run_cross_language_e2e.py            # or: --engram /path/to/engram

# 5 real NEST spiking models over the contract (needs NEST; connects to a running bridge):
#   (in engram) conda run -n engram python -m backend.neurocontrol.bridge_server --backend nest
python3 e2e/nest_five_networks.py                # thin client → 127.0.0.1:28474
```

The cross-language runner stands up engram's `bridge_server --backend mock` (the Python side
of `ncp-gateway`) and drives it from a Python and a Rust client, asserting each completes the
lifecycle with the contract intact. It **skips with a clear message** if a sibling
`engram` checkout isn't found — the two component halves (the Zenoh cross-session test
and the behavioral corpus) still gate on their own.

## `nest_five_networks.py` — 5 real NEST models over one contract

`run_cross_language_e2e.py` answers *"is the contract correct, backend-independent, and
portable across languages?"* by deliberately removing NEST (one `MockBackend`, many
languages). `nest_five_networks.py` answers the **converse**, which a mock by construction
cannot: *does the contract survive contact with a real, heterogeneous neuroscience kernel?*
It drives **five distinct real NEST spiking models** through the **unchanged** NCP RPC
surface (`open_session → step_request* → close_session`) against engram's
`bridge_server --backend nest`.

| # | Network (`network.ref`) | Dynamics it exercises | pop | drive (`current_pA`) |
|---|---|---|---|---|
| 1 | `iaf_psc_alpha`   | current-based LIF, alpha-shaped PSC | 10 | 500 / 750 / 1000 |
| 2 | `iaf_psc_exp`     | current-based LIF, exponential PSC | 10 | 500 / 750 / 1000 |
| 3 | `izhikevich`      | quadratic I&F, regular-spiking regime | 8 | 10 / 15 / 20 |
| 4 | `hh_psc_alpha`    | Hodgkin–Huxley channel kinetics | 6 | 650 / 800 / 1000 |
| 5 | `aeif_cond_alpha` | adaptive exponential I&F, conductance synapses | 6 | 500 / 750 / 1000 |

**Why five model *families*, not one.** A real simulator differs from the mock along exactly
the axes the contract must express but a mock never tests: a numeric integration step
(`dt_ms`), injection of a real **stimulus** (`current_pA` into a named `drive` port),
recording of real **events** (`spikes` out a named `spk` port), and dynamics that span
sub-threshold integration, quadratic reset, full HH channel kinetics, and spike-frequency
adaptation. The *only* thing that varies per network is `network.ref` (plus population size
and drive magnitude); the lifecycle, the `stimulus_frame`, the `records` map, and the
`is_simulation_output` provenance flag are identical across all five. So what is under test is
the **contract's model-agnosticism**, not any one model. Per network it runs three steps of
rising drive and prints total spikes — a smoke-level monotonicity check (more current → more
spikes), **not** a quantitative neuroscience validation.

**It is a thin client (it does not stand up the server).** Unlike the cross-language runner,
which spawns its own mock server on a free port, this script only *connects* —
newline-delimited JSON to `127.0.0.1:28474`, the **default NCP-bridge port** and the same
address the Rust `ncp-gateway` bridges from Zenoh (`NCP_BRIDGE_ADDR`). You must start the
NEST bridge yourself, in an environment where NEST is importable (engram's
`conda run -n engram …`). If nothing is listening it fails fast rather than skipping —
running it is an explicit "I have NEST" choice, which is why it stays out of CI.

**What the medium confirms.** This path is **JSON end-to-end** (the spike records come back as
JSON over the localhost-TCP bridge) — a concrete demonstration that **JSON is the runtime wire**
on the RPC / command / sensor planes. The binary `BulkBlock` is the *observation-plane*
encoding used on Zenoh for bulk numeric data, and the protobuf schema
([`../proto/ncp.proto`](../proto)) is the contract source-of-truth + conformance gate — **not**
this shipped medium. The driver pins `ncp_version="0.5"` and a `contract_hash` inline; that
matches the value the behavior corpus carries today, but unlike `run_cross_language_e2e.py`
(which reads the wire version from the corpus so it tracks a bump) this is a manual sync point
on the next wire cut.

> **Scope.** These runners exercise the **happy-path** lifecycle plus the scientific-boundary
> discriminators. Adversarial and edge-case robustness — e.g. the `bulk.rs` decode memory
> amplification (OOM-DoS), the fail-open unbounded-`ttl_ms` command watchdog, and the
> empty-position geofence bypass — is **not** covered here and is tracked separately in
> [`../KNOWN_LIMITATIONS.md`](../KNOWN_LIMITATIONS.md).

## License

Licensed under either of [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE) at your option.
