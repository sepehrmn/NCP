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

## Threat model & hardening path

The headline risk is a **confused-deputy / world-writable command surface**: on an
open realm, any participant that can reach `…/command` can drive an actuator, and
the local `mode`/`ttl_ms` governor answers "is this command currently valid?" but
not "may this sender command at all?". The fix is authentication + per-plane
authorization, modelled on the established robotics-control-bus mechanisms:

- **Transport auth/ACL** — enable Zenoh access-control + TLS and a per-plane ACL so
  a perception-only client cannot publish to the action plane (cf. **DDS-Security**
  authentication / access-control / cryptographic plugins).
- **Verified controller identity** — bind every action frame to a *proven*
  `controller_id` (mTLS client certs for a closed realm; DID/verifiable-credentials
  for an open realm), and consider per-message signing (cf. **MAVLink 2 message
  signing**) so a replayed or forged command is rejectable.

This is tracked as ROADMAP **P0** (authenticate the action plane) and
[#7](https://github.com/sepehrmn/NCP/issues/7). Until it ships, the closed-realm
guidance above stands.

## Supported versions

The protocol is pre-1.0. Security fixes target the latest released version.

## Reporting a vulnerability

Please report suspected vulnerabilities privately to the maintainer rather than
opening a public issue. Include a description, reproduction steps, and the
affected version. You can expect an acknowledgement and a plan for remediation.
