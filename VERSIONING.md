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

**Pre-1.0 caveat (current):** while `0.x`, a **minor bump is treated as
breaking** — the version guard fails closed on any `0.x` minor difference
(`check_version`). Pin an exact version (`tag = "v0.1.0"`) for anything you build
against. `0.x` is explicitly unstable.

The current wire is **`0.1`** (`ncp_version = "0.1"`); receivers check the
**major** of `ncp_version` for compatibility (see §version negotiation).

## Enforcement: `buf breaking`

Breaking changes are caught mechanically, language-agnostically, by Buf's tiered
rules (configured in `buf.yaml`):

- **`WIRE` / `WIRE_JSON`** — binary and JSON wire compatibility (the contract).
- **`FILE` / `PACKAGE`** — source/codegen-level stability.

CI runs `buf lint`; `buf breaking` against the last released tag gates the wire
across releases (enable once a `v0.2.0` baseline is cut — see `.github/workflows/ci.yml`).
A change that trips `WIRE`/`WIRE_JSON` **must** bump MAJOR (or MINOR while `0.x`).

## Per-session version negotiation (target)

Today the version check is a **local fail-closed reject** (`check_version`). The
target (ROADMAP P1) is an explicit negotiation at `open_session`, modelled on
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
