# Release readiness — NCP wire contract

Status of NCP as a **release-grade, future-extensible wire**: can `v0.5.x` evolve
additively without breaking peers, and is the live medium + contract proven? This is
an honest, adversarially-reviewed assessment, not a green-badge claim.

**Verdict (v0.5.0):** the release-readiness checklist below is **closed**. `0.5` is the
deliberate stable-wire cut — the baseline a stable wire is measured against — and the
control-plane contract is proven end-to-end across a real process + language boundary,
over a real transport, with the safety authority and the version gate exercised on the
wire. NCP remains pre-1.0 (`0.x`, minor-is-breaking) by policy, with the residual
caveats called out at the end — those are *disclosed limitations*, not open blockers.

## Where it stands

The control-plane contract flows end-to-end across a real process + language boundary,
over a real transport, **without NEST or `zenoh-python`** (the backend is separable
from the wire — engram's `MockBackend` emits real `Observation` frames):

- ✅ Two **independent Zenoh sessions** over a real tcp link drive `open→step→run→close`
  through the typed `ZenohNcpClient` + the version/advisory-contract handshake
  (`ncp-zenoh/tests/cross_session_rpc.rs`, readiness-polled).
- ✅ A **Rust** client drives the **real Python** engram server over localhost TCP
  (`e2e/run_cross_language_e2e.py` + `ncp-core/examples/ncp_tcp_client.rs`).
- ✅ engram's **real** `SessionService` serves the lifecycle across a process boundary
  (`Paper2Brain/.../test_e2e_cross_process.py`, ubuntu smoke, NEST-free), with
  forward/backward-compat and malformed/unknown-frame robustness.
- ✅ All four peers decide identically on the contract functions (the behavioral corpus).
- ✅ The scientific-boundary discriminators hold on every reply over every medium.

## The checklist — CLOSED in v0.5.0

| # | Item | Severity | Status |
|---|---|---|---|
| 1 | **Safety governor over the wire** | release-blocking | ✅ `ncp-zenoh/tests/safety_governor_over_wire.rs`: a plant runs `SafetyGovernor::govern` on each `CommandFrame` received over a real Zenoh link; corpus-driven HOLD/ESTOP/clamp verdicts, and the **ESTOP latch survives the wire** (a breach latches; a subsequent clean frame is still ESTOP). |
| 2 | **engram Pydantic enum mirror** | release-blocking | ✅ The six descriptive enums gained `UNKNOWN` + `_missing_` (the Python analogue of `#[serde(other)] Unknown`); `Mode` fail-safes to `HOLD`; `SimMode` still rejects (no Rust counterpart). Round-trip + nested/reply tolerance tests added. |
| 3 | **Frozen JSON-wire baseline gate** | release-blocking | ✅ `scripts/check_wire_baseline.py` + `conformance/baseline/v0.5.0/`: additive-only diff (no removed field/enum-value, no newly-required field, no type change) of CURRENT vs the frozen snapshot. Wired into `scripts/check.sh` + CI. `CommandFrame.mode`/`ControlStatus.mode`/`SimConfig.mode` are now proto enums, so `buf` covers their values too. |
| 4 | **Wire-version single source + mixed-version e2e** | should-fix | ✅ Each peer + the corpus are cross-checked for `NCP_VERSION`/`CONTRACT_HASH` (`behavior_conformance.rs`, `check-version-coherence.sh`); a `0.4` peer is proven fail-closed-rejected by a `0.5` server over the engram cross-process **and** the Zenoh transports, with the `0.5↔0.5` happy path kept. |
| 5 | **new→old reply tolerance + nested unknown field** | should-fix | ✅ Reply-side + nested-message forward-compat tested (Rust + engram Pydantic); a pin asserts no wire model sets `extra='forbid'`. |

**Consciously deferred (nice-to-have, not blocking):** TS + C++ *live-transport* clients
in `e2e/run_cross_language_e2e.py` (cross-language decisions are already proven by the
4-peer behavioral corpus, and live transport by the Rust↔Python e2e), and a
cross-process bulk-codec round-trip (the bulk decoder's hostile-input fail-closed —
bad magic, lying length, allocation-bomb row count — is already comprehensively tested
in-process in `bulk.rs`, and the codec is byte-pinned + conformance-checked). These are
documented here rather than left silent.

## Residual caveats (disclosed limitations, by policy — not open blockers)

- **Pre-1.0 (`0.x`).** The wire may still change; minor-is-breaking, the version guard
  fails closed. Pin `tag = "v0.5.0"`.
- **Single reference implementation.** `proto/ncp.proto` is normative; `ncp-core` (Rust)
  is the reference and the other peers are bindings/mirrors verified by parity + the
  behavioral corpus — not yet a multi-implementation conformance program.
- **The action plane is unauthenticated on an open realm.** The `mode`/`ttl_ms` governor
  is defense-in-depth, not network security. Deploy on a trusted closed realm or enable
  the shipped per-plane Zenoh ACL + mutual TLS (`SECURITY.md`, `ROADMAP.md` P0).

**Bottom line:** the live cross-process loop is real and tested, the forward-compat and
safety properties are proven on the wire, and the whole JSON wire (not just the proto)
is now anchored by a frozen baseline. `v0.5.0` is a sound, interoperable **stable-wire
cut** with its residual limitations disclosed above.
