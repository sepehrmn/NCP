# NCP Versioning & Compatibility Policy

The wire contract carries a string `ncp_version` on every message. This document
is the **published versioning/breaking-change policy** — one of the artifacts a
protocol needs to become a standard (cf. MCP's date-based policy, OMG DDS's
interop program).

## Scheme: SemVer of the wire contract

NCP versions the **wire contract** (`proto/ncp.proto` + the JSON-Schema
projection) with [SemVer](https://semver.org): `MAJOR.MINOR.PATCH`.

- **MAJOR** — a backwards-incompatible wire change (removed/renamed field,
  changed type, removed enum value, changed semantics).
- **MINOR** — a backwards-compatible addition (new optional field, new enum
  value, new message). Existing peers keep interoperating.
- **PATCH** — clarifications/docs with no wire effect.

**Wire version vs crate/package version.** The `ncp_version` *wire* string
(currently `0.2`) versions the contract; the Rust crates and the `@sepehrmn/ncp`
package carry their own SemVer (currently `0.2.4`) for the SDK. They usually move
together, but a PATCH that touches only code/docs/build artifacts (e.g. `0.2.0` →
`0.2.1`) leaves the wire at `0.2`. **Pin `tag = "v0.2.0"`** for the wire baseline
(what the `buf breaking` gate compares against); the crate at that-or-later tag is
wire-`0.2`-compatible.

**Pre-1.0 caveat (current):** while `0.x`, a **minor bump is treated as
breaking** — the version guard fails closed on any `0.x` minor difference
(`check_version`). Pin an exact version (`tag = "v0.2.0"`) for anything you build
against. `0.x` is explicitly unstable.

The current wire is **`0.2`** (`ncp_version = "0.2"`); receivers check the full
`ncp_version` and pre-1.0 require an exact `(major, minor)` match — any `0.x`
minor difference is fail-closed (see §version negotiation). `0.2`
added the neuron-family wire (#10) and the bulk column codec (#6) over `0.1` —
both additive, but a pre-1.0 minor bump, so a `0.1` peer is fail-closed rejected.

## Enforcement: `buf breaking`

Breaking changes are caught mechanically, language-agnostically, by Buf's tiered
rules (configured in `buf.yaml`):

- **`WIRE` / `WIRE_JSON`** — binary and JSON wire compatibility (the contract).
- **`FILE` / `PACKAGE`** — source/codegen-level stability.

CI runs `buf lint`; `buf breaking` gates the wire against the last released tag
(`v0.2.0`, the first proto-bearing baseline — see `.github/workflows/ci.yml`).
A change that trips `WIRE`/`WIRE_JSON` **must** bump MAJOR (or MINOR while `0.x`).

## Per-session version negotiation (target)

Today `check_version` / `negotiate` are provided as **fail-closed library entry
points**: a peer (or gateway) calls `negotiate(peer_version, peer_hash)` at session
setup and refuses a mismatch (reject, never coerce). They are **not yet
auto-invoked on the data-plane receive path** — there is no automatic per-frame or
per-session version rejection wired into the transport, so a version-mismatched
data-plane frame is currently handled by the deserializer (typically a parse
failure / dropped frame), not an explicit version error. The target (ROADMAP P1)
is to wire `negotiate` into an explicit `open_session` handshake, modelled on
MCP's lifecycle:

1. The client sends its `ncp_version` in `OpenSession`.
2. The server, if it cannot serve a compatible major, replies with
   `SessionOpened{ ok: false, error: "unsupported ncp_version: requested <X>, supported <Y>" }`
   and the session does not open (fail closed, never silently coerce).
3. Peers MAY support multiple versions but MUST agree on exactly one per session.

This turns "I refuse" into "we agreed (or explicitly did not)", which is what a
multi-peer protocol needs.

## Deprecation

A field/enum value being retired is first marked deprecated in `proto/ncp.proto`
(a comment + `[deprecated = true]`) for one MINOR cycle before removal in the next
MAJOR, so consumers get a compile-time / lint warning before the break.
