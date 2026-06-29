# Security Policy

## Known limitation: the action/command plane is unauthenticated

The NCP action/command plane is currently **unauthenticated**. On an open realm
it is effectively world-writable: any participant that can reach the realm can
publish action/command messages. There is no transport-level authentication or
authorization on this plane today.

**Deploy NCP only on a trusted, closed realm** (an isolated, access-controlled
Zenoh network). Do not expose the action/command plane on an open or shared
realm.

**The realm is addressing, not authentication.** A "realm" is just the leading
segment of the Zenoh key-expression (`{realm}/session/*/…`, built in
`ncp-core/src/keys.rs`) — a namespace prefix that keeps multiple NCP deployments
from colliding on a shared bus. It is *not* a security boundary: the realm string
is never checked against any credential, so knowing or guessing it grants no
rights and withholding it confers no protection. What actually gates access is the
reachability of the underlying Zenoh network plus the ACL/mTLS below — never the
key prefix. "Closed realm" above therefore means *a closed Zenoh network* (network
isolation, plus the ACL/mTLS that follow), not a secret realm name. Treat the
realm as routing metadata, never as a credential.

### Local fail-safe

As a defense-in-depth fail-safe, action bodies enforce `mode` and `ttl_ms`
locally at the receiver: a stale or out-of-mode command is rejected regardless
of who sent it. This is a local safety governor, **not** a substitute for
network-level authentication.

> **`contract_hash` is not a security control.** The handshake's `contract_hash`
> (`ncp_core::CONTRACT_HASH`) is an FNV-1a digest that *detects accidental contract
> drift* between peers — it is **advisory** (a mismatch is logged, not rejected; see
> `VERSIONING.md`) and is **not** a cryptographic MAC. It provides no integrity or
> authenticity guarantee against an adversary; that is the transport's job (mTLS +
> the ACL below). Do not rely on it as an authentication or anti-tampering gate.

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

**The perception (sensor) plane is an equal attack surface.** Because the
controller computes commands *from* `SensorFrame`s, a participant that can PUBLISH
spoofed sensor data steers the actuator and can defeat the geofence — a
**false-data-injection (FDI)** attack — without ever touching `…/command`. There is
no local-governor equivalent here: sensor-side FDI can be made *perfectly
undetectable* to model-/residual-based monitors (the brain perceives "normal"
operation while the body is driven off trajectory — see Ueda & Kwon,
[arXiv:2408.10177](https://arxiv.org/abs/2408.10177), and Choi & Jang, WISA 2022,
[doi:10.1007/978-3-031-25659-2_14](https://doi.org/10.1007/978-3-031-25659-2_14)),
so a software safety governor is **not** a substitute. The remedy is the same as
for the command plane — **publisher access control**: under DDS-Security / ROS 2
SROS2, *publish* permission is access-controlled per topic independently of
subscribe, on default-deny governance (cf. the **DDS-Security** access-control model
and the SROS2 master governance that enables write access control on every topic).
NCP's ACL template therefore restricts `…/sensor/**` PUT to the `robot` (body)
subject, symmetric to `…/command/**` being restricted to `commander`; both
PUT-authority invariants are mechanically enforced by
[`scripts/check_acl_template.py`](scripts/check_acl_template.py).

This is tracked as ROADMAP **P0** (authenticate the action plane) and
[#7](https://github.com/sepahead/NCP/issues/7). A per-plane Zenoh ACL template
(default-deny; only the authenticated `commander` subject may publish commands, the
robot publishes only its sensors, observers are read-only) is provided at
[`deploy/zenoh-access-control.json5`](deploy/zenoh-access-control.json5) — pair it
with mutual TLS so each subject's identity is proven. Until this ships in a
deployment, the closed-realm guidance above stands.

## Enabling transport authentication (TLS + ACL)

The ACL only binds authorization to identity if that identity is **proven** by
mutual TLS — without mTLS the `cert_common_names` are spoofable and the ACL is
meaningless. To stand up an authenticated realm:

1. **Issue certificates.** Create a CA, then per-subject client certs whose Common
   Names match the ACL `subjects` globs: `commander-service` (the brain, e.g. an
   Engram host), `robot-<id>`/`uav-<id>` (each body), `observer-<id>`/`analysis-<id>` (taps).
   Keep the CA key offline; rotate leaf certs per deployment policy.
2. **Enable mutual TLS on the Zenoh endpoints.** In the router (and every peer)
   config, use a TLS listen endpoint and require client auth — e.g.
   `listen/endpoints: ["tls/0.0.0.0:7447"]` plus the `transport/link/tls` block
   (`root_ca_certificate`, `listen_certificate`, `listen_private_key`, and
   **`enable_mutual_authentication: true`**); each peer presents its
   `connect_certificate`/`connect_private_key`. mTLS is what turns a cert Common
   Name into a *proven* `subjects` match.
3. **Apply the ACL.** Merge [`deploy/zenoh-access-control.json5`](deploy/zenoh-access-control.json5)
   into the router/session config (`zenohd --config …` or the embedded
   `with_config` block). `default_permission: "deny"` rejects anything not
   explicitly allowed.
4. **Verify both PUT invariants.** With the realm up, confirm an `observer`/`robot`
   identity is *rejected* when it `put`s on `…/session/*/command/**` (only `commander`
   succeeds), AND that an `observer`/`commander` identity is *rejected* when it `put`s
   on `…/session/*/sensor/**` (only `robot` succeeds) — both control planes (action
   and perception) are then authenticated, not world-writable.

Schema field names follow the Zenoh 1.x access-control config; validate against
your Zenoh version (authoritative: the zenoh.io configuration docs) before relying
on it. Live mTLS deployment validation is the remaining P0 item on
[#7](https://github.com/sepahead/NCP/issues/7).

### P0 closure checklist (reproducible)

Run this against a *live* mTLS+ACL realm to close P0. Each step states the command
and the **required** outcome; a deployment is P0-validated only when all four hold.
(Substitute your endpoint/cert paths; uses the Zenoh CLI examples `z_put`/`z_sub`.)

| # | As identity (client cert CN) | Action | Required outcome |
|---|---|---|---|
| 1 | `commander-service` | `z_put -k "<realm>/session/s1/command/x" -v '…'` (with commander cert) | **ACCEPT** — the commander may publish commands |
| 2 | `robot-1` / `observer-1` | `z_put -k "<realm>/session/s1/command/x"` (with that cert) | **REJECT** — only the commander may write the action plane |
| 3 | `robot-1` | `z_put -k "<realm>/session/s1/sensor/x"` (with robot cert) | **ACCEPT** — the plant may publish perception |
| 4 | `commander-service` / `observer-1` | `z_put -k "<realm>/session/s1/sensor/x"` | **REJECT** — only the plant may write the perception plane |

Also confirm: a peer presenting **no** client cert (or a CN not in the ACL `subjects`)
is refused at the mTLS layer before any ACL check (connection rejected, not just the
PUT). Record the four outcomes + the no-cert refusal as the P0 evidence; until that
evidence exists, the `SECURITY.md` "closed realm only" guidance stands. The ACL template
itself is CI-guarded for valid tokens + the command/sensor PUT-authority invariants by
[`scripts/check_acl_template.py`](scripts/check_acl_template.py), so a template
regression is caught even though the *live* enforcement test needs a real deployment.

The four-step checklist + the no-cert refusal are automated by
[`scripts/verify_acl_deployment.py`](scripts/verify_acl_deployment.py) — run it
against a live mTLS+ACL realm to produce the P0 evidence in one command. It
exercises both PUT-authority invariants (command and sensor) and the mTLS
no-cert rejection, exiting 0 only when all five hold.

## Residual risks after mTLS + ACL (hardening backlog)

Enabling mutual TLS and the per-plane ACL closes the world-writable command and
perception **PUT** surface (the P0 invariants above). It does **not** make NCP
fully hardened. An adversarial review catalogued the remaining items in
[`KNOWN_LIMITATIONS.md`](KNOWN_LIMITATIONS.md) (none are fixed yet); the
security-relevant ones are summarised below. Do not read this section as a claim
that these are mitigated.

### RPC authorization is all-or-nothing per realm (no per-verb ACL)

The entire session-lifecycle RPC surface is a **single Zenoh queryable key**
(`{realm}/rpc`), so the ACL can only grant or deny `query` on *all* RPC verbs at
once. The shipped template (`deploy/zenoh-access-control.json5`, the
`client-queries-rpc` rule) must let the `robot` and `observer` subjects query that
key — otherwise a default-deny realm could never open a session — which means the
same grant that lets them `open` also lets them `step`, `run`, or **`close` any
session**, not just their own. mTLS proves *who* the caller is, but the transport
ACL matches on the key-expression and cannot see *which verb* is inside the
JSON-RPC body, so it cannot scope it.

Two fixes (see `KNOWN_LIMITATIONS.md`, `zenoh-access-control.json5:81`): split the
privileged verbs onto distinct key-expressions (e.g. `{realm}/rpc/open` vs
`{realm}/rpc/admin`) so the ACL can allow `open` while restricting `step`/`close`
to the `commander` — robust, but **wire-breaking** (it changes the RPC addressing
every peer uses, so it needs a version bump + consumer buy-in); or keep the single
key and have the RPC handler authorize the caller's *proven* mTLS identity per
verb in application code. Until one ships, treat every authenticated client as
able to disrupt any session, and rely on closed-network isolation for the rest.

### Bulk/observation decode is a memory-amplification DoS vector

`BulkBlock::decode` (`ncp-core/src/bulk.rs`) sizes each column's allocation from
attacker-controlled `n_rows`/`data_off` directory fields with **no cumulative
allocation budget**, so a small block whose columns overlap or duplicate can
declare far more total payload than it actually carries — an audited **~64,000x
memory amplification / OOM denial-of-service** (`KNOWN_LIMITATIONS.md`, High). This
is reachable by any peer that can publish on the observation plane, so it is *not*
mitigated by the command/sensor PUT ACL — and bulk/observation data is the binary
`BulkBlock` plane, distinct from the JSON sensor/command frames. The proposed fix
is a running allocation budget bounded by the input length (reject when the summed
declared `data_len` exceeds `bytes.len()`); it is internal and **wire-compatible**,
because every conforming block already lays its columns out disjointly. Until it
lands, ingest bulk data only from trusted publishers.

### The local safety governor has fail-OPEN edge cases

The `mode`/`ttl_ms` governor described under **Local fail-safe** is
defense-in-depth, not authentication — and the audit found inputs that make it
fail *open* rather than closed: an unbounded or non-finite (`+Inf`) `ttl_ms` can
disable the `CommandWatchdog` deadline backstop, and an empty position channel is
treated as the origin and so bypasses the geofence
(`KNOWN_LIMITATIONS.md`, High; `ncp-core/src/safety.rs`). These are
local-enforcement bugs with wire-compatible fixes, but until they are fixed they
weaken the receiver-side fail-safe that the closed-realm guidance leans on.

## Supported versions

The protocol is pre-1.0. Security fixes target the latest released version.

## Reporting a vulnerability

Please report suspected vulnerabilities privately to the maintainer rather than
opening a public issue. Include a description, reproduction steps, and the
affected version. You can expect an acknowledgement and a plan for remediation.
