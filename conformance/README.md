# conformance

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Golden **wire vectors** — canonical NCP message instances every language peer must
round-trip. This directory is the cross-language interop contract: a divergence in any
binding's wire handling fails CI here, not in a downstream integration.

NCP is one normative protocol (`proto/ncp.proto`) with peers in Rust, Python, TypeScript,
and C++. `conformance/vectors/` holds one canonical instance per message `kind` plus the
binary bulk-codec block, so every peer can prove it agrees on the *same* bytes.

## What's here

```text
vectors/*.json   one canonical instance per message kind (open_session, capabilities, …)
vectors/*.bin    packed little-endian bulk column block(s) (bulk_observation.bin)
```

## How the peers consume it

- **Python** — `scripts/check_conformance_vectors.py` validates every `*.json` against
  the schema for its `kind` (field-set + required + enum, resolving local `$ref`/`$defs`)
  and decodes each `*.bin` against its expected columns. Stdlib-only; also gates corpus
  coverage (every schema `kind` must have a vector).
- **C++** — `ncp-cpp/tests/corpus.rs` drives every `*.json` through the C ABI
  `ncp_validate` and asserts accept/reject parity.
- **Rust** — `ncp-core/tests/conformance.rs` guards serde `<->` JSON Schema field-set
  parity type-side; the Rust bulk encoder is byte-pinned to the committed `*.bin`
  (`bulk::tests::matches_committed_golden_vector`).
- **TypeScript** — the `ncp-ts` peer validates the same vectors against the generated
  schemas (see `ncp-ts/`).

Run the whole matrix with `scripts/check.sh` (the conformance corpus step invokes
`check_conformance_vectors.py`).

## See also

- [`NEURO_CYBERNETIC_PROTOCOL.md`](../NEURO_CYBERNETIC_PROTOCOL.md) — the normative spec.
- [repository README](../README.md) — the polyglot SDK and full check matrix.

## License

Licensed under either of [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE) at your option.
