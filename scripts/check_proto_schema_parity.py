#!/usr/bin/env python3
"""Wire conformance: proto <-> JSON-Schema parity guard.

`ncp-core/tests/conformance.rs` already guards the Rust serde types against the
JSON Schemas. This script closes the third side of the triangle: it guards
`proto/ncp.proto` against those same `schemas/*.schema.json`, so the three wire
projections (Rust serde / JSON Schema / protobuf) cannot silently diverge.

What it checks (dependency-free — stdlib only):

  1. FIELD-SET PARITY (hard): for every object in a schema (the top-level
     message *and* every `$defs` object), the proto `message` of the same
     `title` must declare exactly the same set of field names. Catches a renamed,
     added or dropped field on either side.

  2. ENUM WIRE-STRING PARITY (hard, where annotated): for every `$defs` enum,
     the proto `enum` of the same title must annotate each value with its JSON
     wire string (`// wire string "..."`), and that set must equal the schema's
     `enum` array. This is the load-bearing check: proto enum *constants*
     (`V_M`, `CURRENT_PA`) are NOT the JSON wire strings (`"V_m"`, `"current_pA"`),
     so ProtoJSON != the NCP JSON wire for enums — the mapping must be explicit.

  3. PROVENANCE DISCRIMINATORS (hard): `ObservationFrame` and `SimProvenance`
     must carry `calibrated_posterior` and `is_simulation_output` (the
     scientific-boundary fields, per CLAUDE.md / RATIONALE.md).

  4. Reports (non-fatal): schema enums modeled as a plain `string` in proto
     (e.g. `mode`, `SimMode`) and proto enums lacking wire-string annotations,
     so the gaps are visible without failing the build.

Exit code is non-zero on any hard failure. Wire into CI next to the cargo gate.
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
PROTO = REPO / "proto" / "ncp.proto"
SCHEMA_DIR = REPO / "schemas"

# Proto enums whose values intentionally carry no `// wire string` annotation
# because the proto message models the field as a plain `string`, not the enum
# (the enum is defined for typed convenience only). Reported, not failed.
_STRING_MODELED_HINT = "modeled as a `string` field in proto"

# Reverse-pass allowlist: proto messages that intentionally have NO standalone
# JSON Schema (so the proto->schema reverse parity check below doesn't flag them).
# Every entry MUST carry a rationale — the default is that a new proto message
# WITHOUT a schema is a drift bug. The JSON Schemas are generated from the
# backend/neurocontrol Pydantic models (see schemas/index.json), so a proto
# message absent from that model set is either (a) a sub-structure modeled inline
# in a parent schema, or (b) a binary message proven by a different conformance
# path — both must be named here with the reason.
_PROTO_MSG_NO_SCHEMA = {
    # `map<string, ChannelValue>` wrapper. On the JSON wire a SetpointStep is just
    # an object; it is modeled inline as the items of CommandFrame.horizon
    # (`{"type":"object","additionalProperties":{"$ref":"ChannelValue"}}`), so it
    # has no standalone schema `title`.
    "SetpointStep": "map wrapper; modeled inline as CommandFrame.horizon items",
    # Binary bulk message: its payload is a packed little-endian column block
    # (`block` bytes). Conformance is proven by the committed `*.bin` golden
    # vector + the language-agnostic `decode_bulk` reference decoder in
    # check_conformance_vectors.py, not by a JSON-Schema instance. It is
    # deliberately NOT in the Pydantic-generated schema corpus.
    "BulkObservation": "binary block message; proven by *.bin vector + bulk codec",
}


def parse_proto(text: str):
    """Return (messages, enums).

    messages: {Name: [field_name, ...]}
    enums:    {Name: {CONST: wire_string_or_None}}
    """
    messages: dict[str, list[str]] = {}
    enums: dict[str, dict[str, str | None]] = {}
    cur_kind: str | None = None
    cur_name: str | None = None

    field_re = re.compile(r"\b(\w+)\s*=\s*\d+\s*;")
    enum_re = re.compile(r"(\w+)\s*=\s*\d+\s*;(?:\s*//\s*wire string\s*\"([^\"]+)\")?")
    open_re = re.compile(r"(message|enum)\s+(\w+)\s*\{(.*)$")

    def add_message_fields(name: str, body: str) -> None:
        # strip line comments, then pick the identifier before each `= N;`
        for chunk in body.split(";"):
            code = chunk.split("//", 1)[0]
            m = field_re.search(code + ";")
            if m:
                messages[name].append(m.group(1))

    for raw in text.splitlines():
        line = raw.strip()
        if cur_kind is None:
            m = open_re.match(line)
            if not m:
                continue
            cur_kind, cur_name, rest = m.group(1), m.group(2), m.group(3)
            (messages if cur_kind == "message" else enums)[cur_name] = (
                [] if cur_kind == "message" else {}
            )
            if "}" in rest:  # single-line def, e.g. `message SetpointStep { ... }`
                inner = rest[: rest.index("}")]
                if cur_kind == "message":
                    add_message_fields(cur_name, inner)
                cur_kind = cur_name = None
            continue
        # inside a block
        if line.startswith("}"):
            cur_kind = cur_name = None
            continue
        if cur_kind == "message":
            add_message_fields(cur_name, line)
        else:  # enum
            em = enum_re.match(line)
            if em:
                enums[cur_name][em.group(1)] = em.group(2)
    return messages, enums


def walk_schema_objects(schema: dict):
    """Yield (title, kind, payload) for the top-level object and every $defs
    entry. kind is 'object' (payload=set of property names) or 'enum'
    (payload=list of enum values)."""
    def emit(node: dict, name: str | None):
        # The type name is the node's `title` if present (Pydantic puts one inside
        # each object), else the `$defs` KEY (schemars names a def by its key, with no
        # internal `title`). Either projection works; the name is what matters.
        title = node.get("title") or name
        if not title:
            return
        if node.get("type") == "object" and "properties" in node:
            yield title, "object", set(node["properties"].keys())
        elif "enum" in node:
            yield title, "enum", list(node["enum"])
        elif "oneOf" in node:
            # schemars renders an enum whose variants carry doc comments as a `oneOf`
            # of {enum:[...]} and {const:"..."} branches. Gather the wire strings from
            # every branch so enum wire-string parity still covers it (e.g. Observable,
            # StimulusKind, whose binary_state / rate_inject variants are documented).
            values: list[str] = []
            for branch in node["oneOf"]:
                if not isinstance(branch, dict):
                    continue
                if isinstance(branch.get("enum"), list):
                    values.extend(branch["enum"])
                elif "const" in branch:
                    values.append(branch["const"])
            if values:
                yield title, "enum", values

    yield from emit(schema, None)
    for key, node in (schema.get("$defs") or {}).items():
        if isinstance(node, dict):
            yield from emit(node, key)


def main() -> int:
    messages, enums = parse_proto(PROTO.read_text(encoding="utf-8"))

    failures: list[str] = []
    notes: list[str] = []
    checked_objs = 0
    checked_enums = 0

    # De-dup objects/enums shared across schema files (e.g. Observable, ChannelValue).
    seen_obj: dict[str, frozenset] = {}
    seen_enum: dict[str, tuple] = {}
    schema_titles: set[str] = set()  # every object/enum title the schemas define

    for schema_path in sorted(SCHEMA_DIR.glob("*.schema.json")):
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
        where = schema_path.name
        for title, kind, payload in walk_schema_objects(schema):
            schema_titles.add(title)
            if kind == "object":
                fields = frozenset(payload)
                if seen_obj.get(title) == fields:
                    continue
                seen_obj[title] = fields
                if title not in messages:
                    failures.append(
                        f"[{where}] schema object {title!r} has no `message {title}` in proto"
                    )
                    continue
                proto_fields = set(messages[title])
                checked_objs += 1
                missing = fields - proto_fields  # in schema, not in proto
                extra = proto_fields - fields  # in proto, not in schema
                if missing or extra:
                    failures.append(
                        f"[{where}] field-set drift in {title}: "
                        f"missing-from-proto={sorted(missing)} extra-in-proto={sorted(extra)}"
                    )
            else:  # enum
                values = tuple(payload)
                if seen_enum.get(title) == values:
                    continue
                seen_enum[title] = values
                if title not in enums:
                    notes.append(
                        f"[{where}] schema enum {title!r} has no `enum {title}` in proto "
                        f"({_STRING_MODELED_HINT}); wire values={sorted(payload)}"
                    )
                    continue
                proto_vals = enums[title]
                wire = {w for w in proto_vals.values() if w is not None}
                unannotated = [c for c, w in proto_vals.items() if w is None and c.endswith("_UNSPECIFIED") is False]
                if not wire:
                    notes.append(
                        f"[{where}] proto enum {title} carries no `// wire string` "
                        f"annotations; cannot verify against schema {sorted(payload)} "
                        f"(add annotations to enforce)"
                    )
                    continue
                checked_enums += 1
                schema_set = set(payload)
                miss = schema_set - wire
                extra = wire - schema_set
                if miss or extra:
                    failures.append(
                        f"[{where}] enum wire-string drift in {title}: "
                        f"missing-from-proto={sorted(miss)} extra-in-proto={sorted(extra)}"
                    )
                if unannotated:
                    notes.append(
                        f"[{where}] proto enum {title} has unannotated value(s) "
                        f"{unannotated} (no `// wire string`)"
                    )

    # Provenance discriminators must be present on the scientific-boundary types.
    for title in ("ObservationFrame", "SimProvenance"):
        fields = set(messages.get(title, []))
        for disc in ("calibrated_posterior", "is_simulation_output"):
            if disc not in fields:
                failures.append(
                    f"[provenance] proto message {title} is missing the "
                    f"scientific-boundary field {disc!r}"
                )

    # REVERSE PASS (proto -> schema): a proto message with neither a JSON Schema
    # of the same title NOR an allowlist entry is silent drift — the forward pass
    # above can only see schemas, so a proto-only message would otherwise go
    # unnoticed. Symmetric for enums.
    checked_reverse = 0
    for name in sorted(messages):
        if name in schema_titles or name in _PROTO_MSG_NO_SCHEMA:
            checked_reverse += 1
            continue
        failures.append(
            f"[reverse] proto message {name!r} has no JSON Schema and no "
            f"_PROTO_MSG_NO_SCHEMA allowlist entry (add a schema, or allowlist "
            f"it with a rationale if it is intentionally not a JSON-wire message)"
        )
    for name in sorted(enums):
        if name not in schema_titles:
            notes.append(
                f"[reverse] proto enum {name!r} has no schema enum of that title "
                f"(typed-only convenience enum?)"
            )

    print("proto <-> JSON-Schema parity guard")
    print(
        f"  checked {checked_objs} message field-sets, {checked_enums} annotated "
        f"enums, {checked_reverse} proto messages (reverse)"
    )
    if notes:
        print("\n  notes (non-fatal):")
        for n in notes:
            print(f"    - {n}")
    if failures:
        print("\n  FAILURES:")
        for f in failures:
            print(f"    ✗ {f}")
        print(f"\nFAIL: {len(failures)} proto<->schema drift(s).")
        return 1
    print("\nPASS: proto is structurally in sync with the JSON Schemas.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
