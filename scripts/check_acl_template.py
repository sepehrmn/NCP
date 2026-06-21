#!/usr/bin/env python3
"""Structural guard for deploy/zenoh-access-control.json5 (ROADMAP P0, #7).

The per-plane ACL is the one concrete mitigation for the world-writable action
plane, so a template that zenohd silently refuses (or that grants command-put to
the wrong subject) is worse than none — it reads as "secured" while doing nothing.
This guard runs in CI without a Zenoh runtime and fails closed on:

  1. an invalid `messages` token (e.g. the `get` that zenohd rejects — the real
     token for the querier/get side is `query`), and
  2. a violation of either PUT-authority invariant: only the `commander` (brain)
     policy may PUT on the `.../command/**` plane, AND only the `robot` (body)
     policy may PUT on the `.../sensor/**` plane. The perception plane is a control
     input too — a spoofed SensorFrame steers the controller and can defeat the
     geofence (false-data injection) — so it is restricted symmetrically to the
     action plane.

On every run it also self-tests (a tampered template MUST be rejected) so the
guard cannot silently rot into a no-op.

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


def check(cfg: dict) -> list[str]:
    """Return ACL problems (empty == OK). Pure, so it is self-testable."""
    ac = cfg.get("access_control", {})
    rules = ac.get("rules", [])
    policies = ac.get("policies", [])
    errors: list[str] = []

    # (1) every messages token must be a real Zenoh ACL token; track PUT authority
    # on the two control planes (command = action, sensor = perception/FDI input).
    command_put_rules: set[str] = set()
    sensor_put_rules: set[str] = set()
    for rule in rules:
        rid = rule.get("id", "<unnamed>")
        for tok in rule.get("messages", []):
            if tok not in VALID_TOKENS:
                errors.append(
                    f"rule {rid!r}: invalid messages token {tok!r} "
                    f"(zenohd would reject the config; did you mean 'query'?)"
                )
        keys = rule.get("key_exprs", [])
        puts = any(t in ("put", "delete") for t in rule.get("messages", []))
        if puts and any("/command/" in k or k.endswith("/command") for k in keys):
            command_put_rules.add(rid)
        # The perception (sensor) plane is ALSO a control input: a spoofed sensor
        # frame steers the controller and can defeat the geofence (false-data
        # injection). Restrict sensor PUTs to the body, symmetric to command PUTs.
        if puts and any("/sensor/" in k or k.endswith("/sensor") for k in keys):
            sensor_put_rules.add(rid)

    # (2) PUT authority on each control plane is restricted to exactly one subject:
    #     command -> commander (the brain); sensor -> robot (body). These are ROLES,
    #     not project names — the template is project-neutral.
    for pol in policies:
        subjects = set(pol.get("subjects", []))
        for rid in pol.get("rules", []):
            if rid in command_put_rules and subjects != {"commander"}:
                errors.append(
                    f"policy for subjects {sorted(subjects)} binds command-put rule "
                    f"{rid!r}: only 'commander' may publish on the action plane"
                )
            if rid in sensor_put_rules and subjects != {"robot"}:
                errors.append(
                    f"policy for subjects {sorted(subjects)} binds sensor-put rule "
                    f"{rid!r}: only 'robot' (the body) may publish on the perception "
                    f"plane — a spoofed sensor frame is an FDI command channel"
                )

    return errors


def _selftest() -> list[str]:
    """Negative self-tests (stdlib-only): a tampered template MUST be rejected, so
    the guard cannot silently rot into a no-op. Run on every invocation."""
    failures: list[str] = []
    cases = [
        # (description, tampered config, must-be-rejected)
        (
            "a non-robot sensor-put policy",
            {
                "access_control": {
                    "rules": [
                        {
                            "id": "x",
                            "messages": ["put"],
                            "key_exprs": ["ncp/session/*/sensor/**"],
                        }
                    ],
                    "policies": [{"rules": ["x"], "subjects": ["observer"]}],
                }
            },
        ),
        (
            "a non-commander command-put policy",
            {
                "access_control": {
                    "rules": [
                        {
                            "id": "y",
                            "messages": ["put"],
                            "key_exprs": ["ncp/session/*/command/**"],
                        }
                    ],
                    "policies": [{"rules": ["y"], "subjects": ["robot"]}],
                }
            },
        ),
        (
            "an invalid 'get' messages token",
            {
                "access_control": {
                    "rules": [
                        {"id": "z", "messages": ["get"], "key_exprs": ["ncp/rpc"]}
                    ],
                    "policies": [],
                }
            },
        ),
    ]
    for desc, cfg in cases:
        if not check(cfg):
            failures.append(f"{desc} was NOT rejected")
    return failures


def main() -> int:
    self_failures = _selftest()
    if self_failures:
        print("FAIL: ACL guard self-test failed (the guard is broken):", file=sys.stderr)
        for f in self_failures:
            print(f"  - {f}", file=sys.stderr)
        return 1

    text = ACL_PATH.read_text(encoding="utf-8")
    try:
        cfg = load_json5(text)
    except json.JSONDecodeError as e:  # pragma: no cover - structural failure
        print(f"FAIL: could not parse {ACL_PATH.name}: {e}", file=sys.stderr)
        return 1

    errors = check(cfg)
    if errors:
        print("FAIL: ACL template guard found problems:", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 1

    n_rules = len(cfg.get("access_control", {}).get("rules", []))
    print(
        f"OK: {ACL_PATH.name} — {n_rules} rules, tokens valid, "
        f"command-put restricted to commander, sensor-put to robot"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
