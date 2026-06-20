# deploy

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Deployment assets for the [Neuro-Cybernetic Protocol](../NEURO_CYBERNETIC_PROTOCOL.md) (NCP) —
the one normative protocol spoken by its Rust, Python, TypeScript, and C++ peers. This directory
holds the **Zenoh per-plane access-control template**, not application code.

## `zenoh-access-control.json5`

A **default-DENY** Zenoh ACL template (ROADMAP P0, issue #7) that closes the open-realm
action plane: only the authenticated `engram` subject may PUBLISH commands, the `robot`
publishes only sensors and reads commands, and `observer` taps are READ-ONLY. The three
planes (sensor / observation / command) get distinct per-subject permissions, so a
perception-only client can never command.

It is a **template** — adapt `realm`, the `cert_common_names`, and interfaces to your
deployment. Apply it via the Zenoh router/session config:

```bash
zenohd --config deploy/zenoh-access-control.json5
```

or merge it into a `connect`/`listen` config block.

## Open-realm caveat

On an open realm the **realm string is addressing, not a credential** — anyone who can reach
the bus can spoof a subject. This ACL only binds authorization to identity once that identity
is **proven by mutual TLS**: `cert_common_names` are matched by exact string equality, and
without mTLS they are trivially spoofable and the ACL is meaningless. ACL/mTLS is **opt-in**;
see [`SECURITY.md`](../SECURITY.md) for the threat model and the TLS + ACL enablement steps.
The template is shape-checked by `scripts/check_acl_template.py`.

See the [repository README](../README.md) for the full NCP picture and crate map.

## License

Licensed under either of [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE) at your option.
