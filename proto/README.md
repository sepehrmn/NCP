# proto

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **normative protobuf IDL** for the Neuro-Cybernetic Protocol. `ncp.proto` is the
single source of truth for the NCP wire — message structure, field numbers, types, and
the binary encoding.

In the polyglot NCP SDK (one protocol, peers in Rust/Python/TS/C++) every other
representation is generated FROM this file or conformance-checked AGAINST it:

- **language bindings** (Rust/prost, TS/ts-proto, Python, C++) — generated via `buf`
  (`buf.yaml` / `buf.gen.yaml`);
- **JSON Schemas** in `schemas/*.schema.json` — the JSON projection (incl. the enum
  *wire strings* `"V_m"`, `"current_pA"`, …), kept in parity with this file;
- **Rust serde types** in `ncp-core/src/messages.rs` — the reference impl.

Note: not all bindings are mechanically generated yet. The TS `ts-rs` and Python
Pydantic types are **hand-reconciled** against this IDL while the codegen migrates to
fully proto-native. Parity is CI-enforced:
`scripts/check_proto_schema_parity.py` (proto ↔ JSON Schema, field-set + enum
wire-string) and `ncp-core/tests/conformance.rs` (Rust serde ↔ JSON Schema).

## The `ncp_version` axis

Every message carries a `kind` string discriminator and an `ncp_version` string
(currently `"0.2"`). Receivers check the **full** `ncp_version`: pre-1.0 the minor is
breaking, so an exact `(major, minor)` match is required and any `0.x` minor difference
is fail-closed rejected — never coerced. Unknown fields are ignored on deserialize
(additive forward-compatibility within a compatible wire version).

## Editing

```bash
# after editing ncp.proto, re-check wire/JSON parity:
python scripts/check_proto_schema_parity.py
```

> ProtoJSON is **not** the NCP JSON wire for enums: canonical protobuf JSON emits an
> enum's constant name (`"V_M"`), but the NCP JSON wire uses the `schemas/` string
> (`"V_m"`). Each enum value annotates its JSON wire string; protobuf-JSON peers MUST map
> through that table. Binary peers use the field number and are unaffected.

See the [NCP specification](../NEURO_CYBERNETIC_PROTOCOL.md) for the normative protocol
description and the [repository README](../README.md) for the SDK overview.

## License

Licensed under either of [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE) at your option.
