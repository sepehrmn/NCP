#!/usr/bin/env python3
"""Structural guard for deploy/zenoh-access-control.json5 (ROADMAP P0, #7).

The per-plane ACL is the one concrete mitigation for the world-writable action
plane, so a template that zenohd silently refuses (or that grants command-put to
the wrong subject) is worse than none — it reads as "secured" while doing nothing.
This guard runs in CI without a Zenoh runtime and fails closed on:

  1. an invalid `messages` token (e.g. the `get` that zenohd rejects — the real
     token for the querier/get side is `query`), and
  2. a violation of the safety invariant: only the `engram` (commander) policy may
     be bound to a rule that PUTs on the `.../command/**` plane.

It is intentionally a lightweight stdlib-only parse (no json5 dep): it strips `//`
comments, quotes bare keys, and drops trailing commas, which is sufficient for this
template. zenohd remains the authority on the live config; this only catches the
mechanical drift class the review found.
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

# Valid Zenoh 1.x access-control `messages` tokens. `get` is deliberately ABSENT:
# the get/querier side is `query`. Keep this in sync with zenoh's AclMessage.
VALID_TOKENS = {
    "put",
    "delete",
    "declare_subscriber",
    "declare_queryable",
    "query",
    "reply",
    "declare_querier",
    "liveliness_token",
    "declare_liveliness_subscriber",
    "liveliness_query",
}

ACL_PATH = Path(__file__).resolve().parent.parent / "deploy" / "zenoh-access-control.json5"


def load_json5(text: str) -> dict:
    # Strip // line comments (none of this template's strings contain `//`).
    text = re.sub(r"//[^\n]*", "", text)
    # Quote bare object keys: `{ id:` / `, messages:` -> `{ "id":`.
    text = re.sub(r'([{,]\s*)([A-Za-z_][A-Za-z0-9_]*)\s*:', r'\1"\2":', text)
    # Drop trailing commas before } or ].
    text = re.sub(r',(\s*[}\]])', r'\1', text)
    return json.loads(text)


def main() -> int:
    text = ACL_PATH.read_text(encoding="utf-8")
    try:
        cfg = load_json5(text)
    except json.JSONDecodeError as e:  # pragma: no cover - structural failure
        print(f"FAIL: could not parse {ACL_PATH.name}: {e}", file=sys.stderr)
        return 1

    ac = cfg.get("access_control", {})
    rules = ac.get("rules", [])
    policies = ac.get("policies", [])
    errors: list[str] = []

    # (1) every messages token must be a real Zenoh ACL token.
    command_put_rules: set[str] = set()
    for rule in rules:
        rid = rule.get("id", "<unnamed>")
        for tok in rule.get("messages", []):
            if tok not in VALID_TOKENS:
                errors.append(
                    f"rule {rid!r}: invalid messages token {tok!r} "
                    f"(zenohd would reject the config; did you mean 'query'?)"
                )
        # Track rules that PUT on the command plane (the safety-critical authority).
        keys = rule.get("key_exprs", [])
        if any(t in ("put", "delete") for t in rule.get("messages", [])) and any(
            "/command/" in k or k.endswith("/command") for k in keys
        ):
            command_put_rules.add(rid)

    # (2) only the `engram` commander policy may bind a command-put rule.
    for pol in policies:
        subjects = set(pol.get("subjects", []))
        for rid in pol.get("rules", []):
            if rid in command_put_rules and subjects != {"engram"}:
                errors.append(
                    f"policy for subjects {sorted(subjects)} binds command-put rule "
                    f"{rid!r}: only 'engram' (the commander) may publish on the action plane"
                )

    if errors:
        print("FAIL: ACL template guard found problems:", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 1

    print(
        f"OK: {ACL_PATH.name} — {len(rules)} rules, tokens valid, "
        f"command-put restricted to the engram commander"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
