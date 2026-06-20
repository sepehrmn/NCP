# scripts

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Maintenance and ops scripts for the **NCP** workspace — the local conformance gate,
the cross-consumer pin tooling, and the NEST/transport micro-benchmarks. NCP is a
polyglot SDK around **one** normative wire contract with peers in Rust, Python, C/C++,
and TypeScript; these scripts keep that contract checkable and its consumers in sync.

These are operator tools, not part of the published SDK. Run them from anywhere — each
resolves the repo root from its own location.

## Index

| Script | What it does |
|---|---|
| `check.sh` | Local gate: runs the full SDK test matrix across languages (`ncp-core` + TS bindings, `ncp-zenoh` build/loopback, `ncp-gateway`/`ncp-python`/`ncp-cpp` builds, the three Python guards, clippy). Mirrors CI. |
| `repin-ncp.sh <tag> [base-dir]` | Re-pin every NCP consumer (`crebain`, `pid_vla/crates/ncp-observer`, `Paper2Brain`) to a single tag and refresh lockfiles. Edits files only — no commit/push/stage. |
| `check-consumer-pins.sh [expected-tag] [base-dir]` | Read-only pin-consistency guard: reports the NCP pin each downstream consumer references and verifies they agree (optionally against `expected-tag`). No writes, builds, or git/network calls. |
| `check_proto_schema_parity.py` | Wire-conformance guard: `proto/ncp.proto` vs `schemas/*.schema.json` (field-set + enum wire-string parity). |
| `check_conformance_vectors.py` | Validates the golden message vectors in `conformance/vectors/*.json` against the JSON Schemas. |
| `check_acl_template.py` | Structural guard for `deploy/zenoh-access-control.json5` (valid Zenoh tokens + command/sensor PUT-authority invariants). |
| `bench_realtime.py` | Real-time-factor sweep for a Brunel-style NEST network served over NCP. |
| `bench_overlap.py` | Whether in-process Python threading can overlap NCP transport I/O with `nest.Run()` (GIL test). |
| `bench_gil_overlap.py` | Whether a native (non-GIL-holding) thread overlaps transport with `nest.Run()`. |
| `bench_chunk_overhead.py` | Cost of chunked vs monolithic NEST simulation under NCP's stepwise control model. |

## Run

```bash
scripts/check.sh                         # full local gate (mirrors CI)
scripts/check-consumer-pins.sh v0.2.8    # verify all consumers pin v0.2.8
scripts/repin-ncp.sh v0.2.8              # bump all consumers to v0.2.8
python3 scripts/check_proto_schema_parity.py
```

The `bench_*.py` scripts require a working PyNEST install; see `PERFORMANCE.md`.

## See also

- The normative wire contract: [`NEURO_CYBERNETIC_PROTOCOL.md`](../NEURO_CYBERNETIC_PROTOCOL.md)
- Repository overview: [repo README](../README.md)

## License

Licensed under either of [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE) at your option.
