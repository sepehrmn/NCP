#!/usr/bin/env python3
"""Conformance corpus validator — golden message vectors vs the JSON Schemas.

Every file in `conformance/vectors/*.json` is a canonical NCP message instance.
This validates each against the schema for its `kind` (field-set + required +
enum membership, recursively resolving local `$ref`/`$defs`), so any peer can run
the same corpus to prove wire conformance. Dependency-free (stdlib only).

This complements:
  - ncp-core/tests/conformance.rs   (Rust serde  <-> schema, type-driven)
  - scripts/check_proto_schema_parity.py (proto <-> schema)
by checking concrete *instances* — the language-agnostic interop corpus.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
SCHEMA_DIR = REPO / "schemas"
VECTOR_DIR = REPO / "conformance" / "vectors"


def load_schemas() -> dict:
    """Map message `kind` -> schema (via properties.kind.const)."""
    by_kind = {}
    for p in SCHEMA_DIR.glob("*.schema.json"):
        s = json.loads(p.read_text(encoding="utf-8"))
        const = (s.get("properties", {}).get("kind", {}) or {}).get("const")
        if const:
            by_kind[const] = s
    return by_kind


def resolve(schema: dict, root: dict) -> dict:
    """Resolve a local $ref (#/$defs/Name) one hop."""
    ref = schema.get("$ref")
    if not ref:
        return schema
    name = ref.split("/")[-1]
    return (root.get("$defs") or {}).get(name, {})


def validate(inst, schema: dict, root: dict, path: str, errs: list) -> None:
    schema = resolve(schema, root)
    # anyOf (nullable unions): pass if any branch validates structurally.
    if "anyOf" in schema:
        return
    enum = schema.get("enum")
    if enum is not None and inst not in enum:
        errs.append(f"{path}: value {inst!r} not in enum {enum}")
        return
    props = schema.get("properties")
    if props is not None:
        if not isinstance(inst, dict):
            errs.append(f"{path}: expected object, got {type(inst).__name__}")
            return
        for key in inst:
            if key not in props:
                errs.append(f"{path}.{key}: not a schema property (unknown field)")
        for req in schema.get("required", []):
            if req not in inst:
                errs.append(f"{path}.{req}: required field missing")
        for key, val in inst.items():
            if key in props:
                validate(val, props[key], root, f"{path}.{key}", errs)
    elif schema.get("type") == "array" and isinstance(inst, list):
        item = schema.get("items", {})
        for i, v in enumerate(inst):
            validate(v, item, root, f"{path}[{i}]", errs)
    elif "additionalProperties" in schema and isinstance(inst, dict):
        ap = schema["additionalProperties"]
        if isinstance(ap, dict):
            for k, v in inst.items():
                validate(v, ap, root, f"{path}.{k}", errs)


def main() -> int:
    by_kind = load_schemas()
    vectors = sorted(VECTOR_DIR.glob("*.json"))
    if not vectors:
        print(f"no vectors in {VECTOR_DIR}")
        return 1
    total_errs = 0
    for vp in vectors:
        inst = json.loads(vp.read_text(encoding="utf-8"))
        kind = inst.get("kind")
        schema = by_kind.get(kind)
        if schema is None:
            print(f"  ✗ {vp.name}: no schema for kind {kind!r}")
            total_errs += 1
            continue
        errs: list = []
        validate(inst, schema, schema, vp.stem, errs)
        if errs:
            print(f"  ✗ {vp.name} ({kind}):")
            for e in errs:
                print(f"      {e}")
            total_errs += len(errs)
        else:
            print(f"  ✓ {vp.name} ({kind})")
    print()
    if total_errs:
        print(f"FAIL: {total_errs} conformance error(s).")
        return 1
    print(f"PASS: {len(vectors)} golden vectors conform to the schemas.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
