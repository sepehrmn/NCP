# Release readiness — NCP wire contract

Status of NCP as a **release-grade, future-extensible wire** (the question: can v0.4.x
evolve additively without breaking peers, and is the live medum+contract proven?). This
is an honest, adversarially-reviewed assessment, not a green-badge claim.

## Where it stands

The control-plane contract is **proven to flow end-to-end across a real process +
language boundary, over a real transport, without NEST or `zenoh-python`** (the backend
is separable from the wire — engram's `MockBackend` emits real `Observation` frames):

- ✅ Two **independent Zenoh sessions** over a real tcp link drive `open→step→run→close`
  through the typed `ZenohNcpClient` + the version/advisory-contract handshake
  (`ncp-zenoh/tests/cross_session_rpc.rs`, gated by `cargo test`, readiness-polled).
- ✅ A **Rust** client drives the **real Python** engram server over localhost TCP
  (`e2e/run_cross_language_e2e.py` + `ncp-core/examples/ncp_tcp_client.rs`) — a
  crebain/pid_vla-style Rust peer ↔ engram.
- ✅ engram's **real** `SessionService` serves the lifecycle across a process boundary
  (`Paper2Brain/.../test_e2e_cross_process.py`, ubuntu smoke job, NEST-free), with
  forward/backward-compat and **malformed/unknown-frame** robustness (clean error frame,
  framing survives).
- ✅ All four peers decide identically on the contract functions (the behavioral corpus).
- ✅ The scientific-boundary discriminators hold on every reply over every medium.

## Fixed (this round, from the adversarial review)

- **Unknown enum variants are now forward-compatible** — the descriptive wire enums gained
  a `#[serde(other)] Unknown` sentinel (deserialization-only; schema/TS/`CONTRACT_HASH`
  unchanged), so an additive enum value within a wire version no longer hard-rejects older
  peers. *(Rust reference + the `ncp-python` binding, which inherits it via serde.)*
- **Flaky sleep → readiness poll** in the Zenoh e2e; **malformed-frame** test added;
  **cross-language runner** no longer passes green on a cargo build failure (only a missing
  toolchain skips).

## Remaining before a stable-wire (1.0) release — the checklist

The suite proves **interoperability at HEAD**, not yet that v0.4.x **stays** wire-stable:
today only one oracle is *frozen* (`buf breaking` vs tag `v0.4.0`, proto-only); the
schemas, golden vectors, behavior corpus, and per-language constants all **regenerate from
/ track the reference**, so a wire-break expressible outside the proto has no frozen anchor.

| # | Item | Severity | Concretely |
|---|---|---|---|
| 1 | **Safety governor over the wire** | release-blocking | The HOLD/ESTOP/clamp authority is only unit-tested in-process. Add a cross-session ncp-zenoh test: a `plant` runs `SafetyGovernor::govern` on each received `CommandFrame`; a `controller` publishes a geofence-breaching sensor → assert the command on the wire is latched ESTOP+zeroed; stale sensor → HOLD; over-limit → clamp. |
| 2 | **engram Pydantic enum mirror** | release-blocking | The Python *binding* inherits the enum fix, but engram's independent Pydantic enums (`session.py`/`protocol.py`) still hard-reject unknown values. Add a `_missing_` classmethod returning an `UNKNOWN` member to each wire enum (the Python analogue of `#[serde(other)]`). |
| 3 | **Frozen v0.4.0 JSON-wire baseline gate** | release-blocking | Freeze a `v0.4.0` snapshot of `schemas/` + golden vectors + the required-field lists + `NCP_VERSION` + the enum/mode wire-string sets, and add a CI gate diffing CURRENT-vs-FROZEN (additive-only within a wire version) — extending buf's proto-only guarantee to the whole JSON wire. Promote `CommandFrame.mode` to the proto `Mode` enum so buf covers its values. |
| 4 | **Wire-version single source + mixed-version e2e** | should-fix | `NCP_VERSION` is two independent hardcoded `"0.4"` constants (Rust + Python). Assert each peer's `NCP_VERSION == conformance/behavior/vectors.json.ncp_version` (the corpus already does this for `contract_hash`); add a real mixed-version e2e (server bumped to 0.5, 0.4 client rejected). |
| 5 | **new→old reply tolerance + nested unknown field** | should-fix | Forward-compat is tested only request-side and top-level. Add: a reply carrying a future field (old client still decodes); an unknown field in a *nested* message; a Python test pinning `extra='ignore'` (a careless `deny_unknown_fields`/`extra='forbid'` would silently break every peer). |
| 6 | **TS + C++ live-transport clients** | nice-to-have | TS/C++ peers only run the decision corpus; add a TS (node) and C++ (C-ABI) client to `run_cross_language_e2e.py` so all four peers are driven over a live transport. |
| 7 | **Bulk observation codec cross-process** | nice-to-have | `ncp-core::bulk` is byte-pinned in-process only. Round-trip a `BulkBlock` across the wire, and feed a hostile block (bad magic, lying length, allocation-bomb row count) to confirm fail-closed, not crash. |

**Bottom line:** the live cross-process loop is real and tested, and the biggest
forward-compat hole (unknown enum variants on the reference) is fixed. Two release-blockers
(the safety governor over a real transport, and the engram Python enum mirror) and one
structural gate (a frozen JSON-wire baseline) remain before NCP should be tagged as a
**stable** wire. Until then it is a sound, interoperable **pre-1.0** wire.
