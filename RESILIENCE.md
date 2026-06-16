# NCP over a degraded link — resilience design (poor connection *and* jamming)

Researched against June-2026 SOTA (streaming/erasure codes, the information theory
of networked control, Age-of-Information / semantic communication, anti-jamming +
predictive control) and then **adversarially pruned**. The honest result is small:
most of the literature is overkill for a 3–12-float control stream. What survives
is high-value and fits NCP's existing `seq`/`ttl_ms`/`mode`/codec.

This covers **general link degradation** — random loss, latency/jitter, low
bandwidth, intermittent connectivity — with **jamming as the adversarial worst
case**, not the only case.

## Per-plane threat model

- **Perception plane** (`SensorFrame`, plant→controller; DROP +
  conflate, reliability left at Zenoh default): *lossy-OK* — a dropped sample is fine if a fresher one arrives, until
  arrival probability crosses a floor. The pressure here is **low bandwidth**
  (what to send when you can't send everything) and freshness.
- **Action plane** (`CommandFrame`, controller→plant; express + DROP + RealTime):
  *safety-critical, low-rate* — a command is useful only if it arrives within its
  deadline; a late frame is a dropped frame, and a *burst* of drops over a fast
  unstable mode lets the state escape.
- **Control-RPC plane** (lifecycle; Reliable/Block): rare, not real-time — ARQ is
  correct, no change.

## Review finding (fix first): `ttl_ms` is currently dead metadata

`CommandFrame.ttl_ms` is carried on the wire and documented as ≡ DDS LIFESPAN, but
**no code enforces it** — the `SafetyGovernor` only checks *sensor* staleness
(`command_timeout_ms`, default 500 ms), and a typical robot/UAV actuator has no command-age
check. Every resilience idea below assumes the deadline backstop exists, so
**enforcing `ttl_ms` plant-side is item 0** (a `CommandWatchdog` primitive now
ships in `ncp-core::safety` — see below — so the plant can HOLD on an expired or
missing command).

## The layered design (what survives the pruning)

### Layer 0 — feasibility, honestly
One constant is genuinely actionable: the **Sinopoli critical arrival probability**
`p_c = 1 − 1/|λ_max|²` (Kalman with intermittent observations) — the perception
floor; DROP-conflate is correct while measured arrival `p̂ > p_c`. The **data-rate
theorem** `R_min = Σ log₂|λᵢ|` matters only as a *goodput-collapse sentinel*: at
20–50 Hz × tens of bytes NCP is rate-rich by ~3 orders of magnitude, so `R_min`
*sizes nothing* — it only tells you when the link has effectively died and you must
fail safe. **Anytime capacity** (Sahai–Mitter) correctly motivates "use a causal/
streaming scheme, not block FEC," but for a 1–3-symbol payload it's motivation, not
a redundancy formula. (Honest: three of the four classic thresholds are rigor; one,
`p_c`, binds.)

### Layer 1 — action plane: packetized predictive control (the one real win)
Each `CommandFrame` carries a short **horizon** of future setpoints, not one. The
actuator buffers them and, on a dropout, **replays the buffered prediction** for
that tick — a single lost packet becomes a non-event, using only the `seq` already
present. Overlapping horizons re-send predictions for neighbouring ticks, so a
*burst* is ridden out without parity overhead (the anytime-causal structure for
free). The NEST controller emits a horizon by rolling its readout forward N ticks.

**Safety invariant (load-bearing):** replaying a stale predicted command is
open-loop dead-reckoning — if a disturbance hits during the blackout it actively
commands the wrong thing and diverges on an unstable mode. Therefore **N is capped
at `ttl_ms / horizon_dt_ms`**, each horizon entry `i` expires at
`t + i·horizon_dt_ms`, and once the buffer drains or any entry is past `ttl_ms`,
**HOLD fires**. The whole safety argument rests on this cap.

No RS / RLNC / RaptorQ / streaming-FEC module: for a 3–12-float setpoint, PPC's
overlapping horizons plus optional **whole-frame duplication** (adaptive, cheap) is
the complete application-layer redundancy story. Coding theory says nothing better
exists for K≈1–3 symbols.

### Layer 2 — perception plane under low bandwidth: PID-informed Value-of-Information
This is where **Partial Information Decomposition** earns its place (see the PID
section below): drop **redundant** channels, keep **unique** ones, bundle
**synergistic** ones — a principled "what to send when you can't send everything,"
designed offline (via an information-theoretic analysis client) and applied online as static channel priorities.
Conflation stays as the wire backstop; a `max_silence_ms` heartbeat distinguishes
"no change" from "link dead."

### Layer 3 — detection & fail-safe
A lightweight detector in `ncp-core` over the **`seq`-gap** stream (already on both
planes): loss rate + a **CUSUM change-point** test (minimum-delay detection) to
separate random loss (poor connection) from a sustained burst (jamming), published
as a `LinkStatus` telemetry message. The fail-safe is the point: when `p̂ < p_c` or
goodput collapses toward `R_min`, escalate **HOLD → ESTOP** (the only two `mode`
rungs that exist today; an autonomous-RTL rung would need a new `Mode` variant + a
MAVROS SET_MODE path that a given robot/UAV client may not yet have — out of scope until built).

**The hard PHY boundary, stated plainly:** no application-layer scheme — not PPC,
not duplication, not coding — recovers data when a wideband jammer drives delivered
goodput to ~0 for longer than the PPC horizon. App-layer mitigates *partial/burst*
loss only; it buys exactly `N · horizon_dt_ms` of ride-through and nothing more.
Frequency-hopping/DSSS is the radio's job. Under a sustained full-band jam the
*only* correct behavior is the fail-safe — **detect goodput collapse and fail safe
honestly**, do not pretend more redundancy helps.

## Is PID (Partial Information Decomposition) useful here, beyond Shannon?

**Yes — as an offline design tool for the perception plane, complementary to
Shannon information theory.** The two answer different questions:

- **Shannon / channel & control info theory** (data-rate theorem, capacity, AoI,
  Sinopoli `p_c`) sizes the link and decides *whether* control is feasible and
  *how much* reliability you need — it is plane-agnostic about *content*.
- **PID** decomposes the information that the sensor channels {S₁…Sₙ} *jointly*
  carry about the control target/action into **Unique**, **Redundant**, and
  **Synergistic** atoms (Williams–Beer and successors). That is exactly the missing
  half under a poor (low-bandwidth) connection: *which channels to send, drop, or
  replicate*:
  - **Redundant** info across channels → safe to drop/compress the redundant ones
    under a bandwidth squeeze without losing control-relevant information (the
    cheapest, safest rate cut).
  - **Unique** info → must be preserved; each unique-bearing channel is
    irreplaceable.
  - **Synergistic** info → channels must travel *together* (dropping one destroys
    the synergy) — tells you what to bundle and co-prioritize.
  - Conversely, to *gain* loss-robustness you can deliberately add **redundant**
    encodings, with PID quantifying how much robustness each costs in bandwidth.

  This operationalizes "Value of Information" *per source*, which raw mutual
  information cannot do (MI gives totals, not the unique/redundant/synergistic
  split).

**The elegant part:** an information-theoretic analysis client (an `ncp-observer`
crate) *already* computes PID on the `(V,L,D,A)` tap of NCP. So the loop closes —
the analysis client measures the PID structure of
{sensor channels → action} **offline**, and feeds back a channel
priority/drop/replicate policy that the perception codec applies **online**.
NCP ↔ the analysis client becomes a closed design loop.

**Honest caveats (from PID's own domain):** PID is computationally expensive
(the redundancy lattice is super-exponential; estimating it from finite,
continuous, high-dimensional data is hard — bias, estimator choice, the
`I_min`-vs-alternatives debate — which is an open research problem). So
PID is **offline / design-time**, never a per-tick online computation: you run it to
*set* static channel priorities, then apply those cheaply online. It informs the
codec; it is not a runtime control primitive.

## Honest scope

**Build (high-value, auditable, fits existing fields):** enforce `ttl_ms`
(shipped: `CommandWatchdog`); PPC horizon capped by `ttl_ms`; seq-gap + CUSUM
detector + `LinkStatus`; staged fail-safe HOLD→ESTOP gated on `p_c` + goodput
collapse; PID-informed perception priorities (offline analysis client → online codec);
whole-frame duplication as an adaptive lever.

**Do not build (overkill / redundant / unimplementable on current code):** RS /
CRLNC / streaming-FEC modules and RaptorQ (overkill for K≈1–3 symbols, redundant
with PPC); event-triggered/send-on-delta sampling (fights the existing DROP+conflate
+ rate-codec design); layered/scalable coding (no maps/trajectories on the bus);
deep-RL / game-theoretic anti-jam (its levers are PHY, not NCP); an autonomous-RTL
mode rung (needs MAVROS SET_MODE wiring that doesn't exist); using `H`/anytime as
redundancy-sizing formulas (rigor-theater for this payload); a client's own
KF/EKF/UKF state estimator is **not** wired into the NCP path, so don't predicate AoII on it.

**The bottom line the theory insists on:** these are *feasibility and fail-safe*
criteria — not a stability certificate for the SNN controller, whose effective
closed-loop decay rate must be *measured*, not assumed. Under a strong jam the most
important thing NCP can do is detect goodput collapse and fail safe honestly.

## Minimal first implementation (corrected order)

0. **Enforce `ttl_ms`** plant-side (`CommandWatchdog` in `ncp-core::safety` — done;
   wire it into the actuator handler).
1. **PPC horizon** on `CommandFrame` (`horizon` field), actuator buffer keyed on
   `seq`, **N ≤ ttl_ms/horizon_dt_ms**, per-entry expiry, HOLD on drain.
2. **seq-gap + CUSUM detector + `LinkStatus`** telemetry.
3. **Staged SafetyGovernor**: HOLD→ESTOP on `p̂<p_c` / goodput collapse (no RTL rung
   until the `Mode` variant + MAVROS path exist).
4. **PID-informed perception priorities** (offline via an analysis client) + optional adaptive
   duplication.

No new dependencies, no wire-breaking edits — additive `Option`/`Vec` fields and
`ncp-core` logic only.
