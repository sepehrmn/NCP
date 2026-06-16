# Security Policy

## Known limitation: the action/command plane is unauthenticated

The NCP action/command plane is currently **unauthenticated**. On an open realm
it is effectively world-writable: any participant that can reach the realm can
publish action/command messages. There is no transport-level authentication or
authorization on this plane today.

**Deploy NCP only on a trusted, closed realm** (an isolated, access-controlled
Zenoh network). Do not expose the action/command plane on an open or shared
realm.

### Local fail-safe

As a defense-in-depth fail-safe, action bodies enforce `mode` and `ttl_ms`
locally at the receiver: a stale or out-of-mode command is rejected regardless
of who sent it. This is a local safety governor, **not** a substitute for
network-level authentication.

## Supported versions

The protocol is pre-1.0. Security fixes target the latest released version.

## Reporting a vulnerability

Please report suspected vulnerabilities privately to the maintainer rather than
opening a public issue. Include a description, reproduction steps, and the
affected version. You can expect an acknowledgement and a plan for remediation.
