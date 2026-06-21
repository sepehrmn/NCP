#!/usr/bin/env python3
"""Frozen JSON-wire baseline gate (RELEASE_READINESS blocker #3).

`buf breaking` freezes the *protobuf* wire against a tagged baseline. But NCP's
actual transport is serde/Pydantic **JSON**, and a break expressible only in the
JSON projection (a removed field, a field that became required, a removed enum
value, a changed type) has no frozen anchor — every other oracle (`schemas/`, the
golden vectors, the behavior corpus, the per-language constants) *regenerates from*
the reference, so it tracks HEAD rather than pinning it.

This gate closes that hole. It distills the load-bearing JSON-wire shape from
`schemas/*.schema.json` — per message-kind field set + which fields are required +
each field's structural type, plus every enum's wire-string value set (the
deserialize-only `unknown` sentinel is `schemars(skip)`, so it is already absent) —
and diffs the CURRENT distillation against a FROZEN snapshot under
`conformance/baseline/v<wire>.0/`. The rule is **additive-only within a wire
version**:

  FAIL  removed kind / removed field / field type changed / field became required /
        removed enum value / the wire version itself changed (freeze a new baseline)
  OK    new kind / new optional field / new enum value (forward-compatible additions)

stdlib only (no jsonschema dep), so it runs anywhere `check.sh` does.

Usage:
  scripts/check_wire_baseline.py                 # diff CURRENT vs the frozen baseline
  scripts/check_wire_baseline.py --freeze DIR    # write a new frozen baseline to DIR
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
SCHEMAS = REPO / "schemas"
MESSAGES_RS = REPO / "ncp-core" / "src" / "messages.rs"
GOLDEN_VECTORS = REPO / "conformance" / "vectors"

# Structural keys that define a field's WIRE type. Everything else in a JSON-Schema
# node (description, default, title, examples) is cosmetic and must NOT trip the gate.
_STRUCT_KEYS = ("type", "$ref", "const", "enum", "format")


def _structural(node):
    """A cosmetic-insensitive signature of a JSON-Schema node's *type*."""
    if not isinstance(node, dict):
        return node
    out = {k: node[k] for k in _STRUCT_KEYS if k in node}
    if isinstance(node.get("items"), dict):
        out["items"] = _structural(node["items"])
    ap = node.get("additionalProperties")
    if isinstance(ap, dict):
        out["additionalProperties"] = _structural(ap)
    for union in ("anyOf", "oneOf", "allOf"):
        if union in node:
            out[union] = [_structural(x) for x in node[union]]
    return out


def _type_repr(field_schema) -> str:
    return json.dumps(_structural(field_schema), sort_keys=True)


def _enum_values(ddef) -> list | None:
    """Extract a string-enum's wire-value set from a `$defs` node, or None if it is
    not a string enum. schemars emits two shapes: a plain `{"enum": [...]}` (e.g.
    Mode, SimMode) and — when variants carry doc-comments or a skipped `Unknown` —
    a `{"oneOf": [{"enum": [...]}, {"const": "..."}]}` of string members (the six
    descriptive enums). Collect values from both so a removed value is caught either
    way; the skip-only `unknown` sentinel is already absent from the schema."""
    if isinstance(ddef.get("enum"), list):
        return list(ddef["enum"])
    one = ddef.get("oneOf")
    if isinstance(one, list) and one and all(
        isinstance(m, dict) and m.get("type") == "string" and ("enum" in m or "const" in m)
        for m in one
    ):
        vals: list = []
        for m in one:
            vals.extend(m["enum"] if "enum" in m else [m["const"]])
        return vals
    return None


def _wire_pins() -> tuple[str, str]:
    """Read NCP_VERSION + CONTRACT_HASH from the Rust reference (single source)."""
    text = MESSAGES_RS.read_text()
    ver = re.search(r'NCP_VERSION:\s*&str\s*=\s*"([^"]+)"', text)
    h = re.search(r'CONTRACT_HASH:\s*&str\s*=\s*"([^"]+)"', text)
    if not ver or not h:
        sys.exit(f"ERROR: could not read NCP_VERSION/CONTRACT_HASH from {MESSAGES_RS}")
    return ver.group(1), h.group(1)


def build_manifest(schemas_dir: Path) -> dict:
    """Distill the JSON-wire shape from a directory of *.schema.json files."""
    ncp_version, contract_hash = _wire_pins()
    kinds: dict[str, dict] = {}
    enums: dict[str, list[str]] = {}
    for p in sorted(schemas_dir.glob("*.schema.json")):
        s = json.loads(p.read_text())
        props = s.get("properties") or {}
        kind = p.name[: -len(".schema.json")]  # Path.stem leaves a stray ".schema"
        kinds[kind] = {
            "fields": {name: _type_repr(fdef) for name, fdef in props.items()},
            "required": sorted(s.get("required") or []),
        }
        for dname, ddef in (s.get("$defs") or {}).items():
            if not isinstance(ddef, dict):
                continue
            ev = _enum_values(ddef)
            if ev is not None:
                # Last write wins; an enum's value set is identical wherever it appears
                # ($defs are the same schemars-emitted definitions across schemas).
                enums[dname] = sorted(ev)
    return {
        "ncp_version": ncp_version,
        "contract_hash": contract_hash,
        "kinds": kinds,
        "enums": enums,
    }


def diff(frozen: dict, current: dict) -> list[str]:
    """Additive-only diff: list the BREAKING changes (empty list = compatible)."""
    fails: list[str] = []

    if current["ncp_version"] != frozen["ncp_version"]:
        fails.append(
            f"wire version changed {frozen['ncp_version']!r} -> {current['ncp_version']!r}: "
            f"that is a NEW wire line — freeze a new baseline (conformance/baseline/"
            f"v{current['ncp_version']}.0) rather than mutating this one"
        )

    fk_all, ck_all = frozen["kinds"], current["kinds"]
    for kind, fk in fk_all.items():
        ck = ck_all.get(kind)
        if ck is None:
            fails.append(f"kind {kind!r} was REMOVED (breaking)")
            continue
        for fname, ftype in fk["fields"].items():
            if fname not in ck["fields"]:
                fails.append(f"{kind}.{fname} field was REMOVED (breaking)")
            elif ck["fields"][fname] != ftype:
                fails.append(
                    f"{kind}.{fname} TYPE changed (breaking): {ftype} -> {ck['fields'][fname]}"
                )
        for r in sorted(set(ck["required"]) - set(fk["required"])):
            fails.append(
                f"{kind}.{r} became REQUIRED (breaking: an older peer that omits it now fails)"
            )

    for ename, evals in frozen["enums"].items():
        cvals = current["enums"].get(ename)
        if cvals is None:
            fails.append(f"enum {ename!r} was REMOVED (breaking)")
            continue
        for v in sorted(set(evals) - set(cvals)):
            fails.append(f"enum {ename} value {v!r} was REMOVED (breaking)")

    return fails


def freeze(dest: Path) -> int:
    manifest = build_manifest(SCHEMAS)
    dest.mkdir(parents=True, exist_ok=True)
    (dest / "wire_manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    # Audit snapshot: the exact schemas + golden vectors this baseline distills, so the
    # frozen wire is human-auditable, not just a derived manifest.
    for sub, src in (("schemas", SCHEMAS), ("vectors", GOLDEN_VECTORS)):
        out = dest / sub
        if out.exists():
            shutil.rmtree(out)
        shutil.copytree(src, out)
    print(
        f"FROZE wire baseline {manifest['ncp_version']} (hash {manifest['contract_hash']}) "
        f"-> {dest.relative_to(REPO)} : {len(manifest['kinds'])} kinds, {len(manifest['enums'])} enums"
    )
    return 0


def check() -> int:
    ncp_version, _ = _wire_pins()
    baseline_dir = REPO / "conformance" / "baseline" / f"v{ncp_version}.0"
    frozen_path = baseline_dir / "wire_manifest.json"
    if not frozen_path.exists():
        print(
            f"ERROR: no frozen wire baseline for the current wire {ncp_version} at "
            f"{frozen_path.relative_to(REPO)}.\n"
            f"  Freeze it once the v{ncp_version}.0 wire is final:\n"
            f"    python3 scripts/check_wire_baseline.py --freeze "
            f"conformance/baseline/v{ncp_version}.0",
            file=sys.stderr,
        )
        return 1
    frozen = json.loads(frozen_path.read_text())
    current = build_manifest(SCHEMAS)
    fails = diff(frozen, current)
    if fails:
        print(f"WIRE BASELINE BREAK vs frozen v{ncp_version}.0 ({len(fails)} breaking change(s)):", file=sys.stderr)
        for f in fails:
            print(f"  - {f}", file=sys.stderr)
        print(
            "\nThese are NOT additive. Within a wire version the JSON wire may only grow "
            "(new kind / new optional field / new enum value). A genuine break requires a "
            "wire-version bump + a new frozen baseline.",
            file=sys.stderr,
        )
        return 1
    print(
        f"PASS: JSON wire is additively compatible with the frozen v{ncp_version}.0 baseline "
        f"({len(frozen['kinds'])} kinds, {len(frozen['enums'])} enums)."
    )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Frozen JSON-wire baseline gate (additive-only).")
    ap.add_argument("--freeze", metavar="DIR", help="write a new frozen baseline to DIR")
    args = ap.parse_args()
    if args.freeze:
        return freeze(Path(args.freeze) if Path(args.freeze).is_absolute() else REPO / args.freeze)
    return check()


if __name__ == "__main__":
    raise SystemExit(main())
