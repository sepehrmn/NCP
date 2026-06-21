# Can NCP read NEST in real time, without stopping the simulation (like MUSIC)?

**Short answer: yes.** NCP exchanges data with a *running* NEST kernel without
tearing it down or rebuilding the network — using NEST's own
`Prepare()` → `Run(chunk)` → `Cleanup()` stepwise model with a persistent kernel.
That is the same **boundary-exchange** model MUSIC uses; the difference from MUSIC
is the *deployment shape* (MUSIC co-schedules multiple simulators in one MPI world
with a shared clock; NCP serves one NEST instance to remote, multi-language
clients), **not** the ability to interact with a live simulation.

This is checked 10 ways below against the reference NEST backend (the host
simulation service's `SimulationBackend` implementation and the `NeuroControlLoop`
in `ncp-core`).

### 1. NEST execution model — does NCP use stepwise `Run`, or `Simulate`-and-stop?
The reference backend's `open()` calls `nest.Prepare()` **once**; each `step()`
calls `nest.Run(advance_ms)`; `close()` calls `nest.Cleanup()`. This is NEST's
documented stepwise/continuous-interaction mode — the same mode the NEST Server and
co-simulation frameworks use — not a `Simulate()`-teardown loop.

### 2. Kernel-state persistence — is the network rebuilt between reads?
`ResetKernel()` happens **only at `open()`**. Between `step()`s
there is no reset and no re-`Create`; the populations, recorders, generators and
all neuron state persist. Simulation time advances monotonically across chunks
(`sim_time_ms += advance_ms`). So "without stopping the simulation" holds in the
sense that matters: no teardown, no rebuild, continuous state.

### 3. Data readback — streaming deltas or re-reading history?
The reference `step()` reads each recorder's events and slices `[last:]`, returning
**only the events since the previous step** — a streaming delta per chunk. The
reference control loop goes further and reads `n_events` counts, computing rate from
the **count delta** (O(1), no event array). Either way you get new data each tick
from the live kernel. For high-volume raw spike/`V_m` streaming, the observation
plane can carry the deltas as a packed little-endian column block (`ncp-core::bulk`,
proto `BulkObservation`) instead of `repeated double` — parse-free and ~2× smaller;
an additive, observation-plane-only option (see [`PERFORMANCE.md`](PERFORMANCE.md), #6).

### 4. Runtime input injection — can you stimulate mid-simulation?
Yes. Each `step()` sets generator parameters (`dc_generator.amplitude`,
`poisson_generator.rate`) **before** the `Run` chunk, so a controller drives the
running network in real time — the inverse of MUSIC's input event ports.

### 5. Granularity — continuous, or quantized?
Quantized, like MUSIC. NCP exchanges at `chunk_ms` (`SimConfig.chunk_ms`)
boundaries; MUSIC exchanges at its tick interval. Both are discrete; both let you
choose the interval. The ROS-MUSIC toolchain's own latencies (≈70 ms at a 1 ms
tick … ≈350 ms at a 50 ms tick) are exactly this tick/chunk trade-off.

### 6. Concurrency — can you read *during* a Run (mid-tick)?
No — and neither can MUSIC. `nest.Run(chunk)` is a synchronous, blocking advance;
you read/inject between Runs. MUSIC likewise services its ports at tick
boundaries, not mid-tick. Same model: the simulator and the exchange interleave at
boundaries; they do not run truly concurrently with the read.

### 7. Real-time pacing — does it keep up with wall-clock?
`NeuroControlLoop` (in `ncp-core`) sleeps to hold a target `rate_hz`, pacing to
wall-clock when NEST is faster than real time. For large networks NEST may run
*slower* than real time — a property of the model size, identical for MUSIC (and
why neuromorphic on-chip wins on raw loop latency). Neither MUSIC nor NCP
*guarantees* real time; both provide *runtime* exchange.

### 8. Transport & reach — local only, or networked/multi-language?
MUSIC's runtime exchange is **MPI**, within one co-launched (possibly multi-node)
allocation, C++/Python. NCP carries the same per-chunk exchange over **Zenoh / a
localhost gateway / WebSocket**, to **remote, heterogeneous, multi-language**
clients (Rust/Python/TS/C++). This is NCP's advantage for the "serve one NEST sim
to a fleet + observers" case.

**On latency, the common intuition is backwards.** MUSIC is *not* a low-microsecond
shared-memory hop — it uses **buffered pairwise `MPI_Send`/`MPI_Recv`** (Djurfeldt et
al., *Neuroinformatics* 2010, [PMC3240549](https://pmc.ncbi.nlm.nih.gov/articles/PMC3240549/)),
and its *closed-loop* latency is buffering- and tick-bound: the ROS-MUSIC measurements
(Weidel et al., *Front. Neuroinform.* 2016, 10:31) report ≈**70 ms at a 1 ms tick**,
rising ≈linearly to ≈**350 ms at a 50 ms tick**, and compounding with the number of
hops in the loop. NCP's per-*exchange* transport is ≈0.1 ms (Zenoh loopback) to
≈0.2–1 ms (WebSocket+JSON) — one to two orders of magnitude **under** MUSIC's
closed-loop floor. So for a single `sensor → NEST → actuator` reaction, NCP is **not
slower than MUSIC — if anything faster**. MUSIC's real edge is elsewhere (§9 and the
"use MUSIC, not NCP, when" box below): synchronizing *several* simulators on one clock,
and bulk intra-HPC spike throughput via collective MPI. The one honest latency
asymmetry that *does* favour MUSIC is structural, not transport: because Python is
NEST's only binding, NCP pays a PyNEST/SLI round-trip per chunk (~0.1 ms of fixed
host overhead, measured ~107 µs per `device.set`, ~17.5 µs per `get` on NEST 3.9),
which puts a soft floor of ≈1–2 ms on `chunk_ms` in the small-network regime — a floor
MUSIC's C++/MPI tick does not have. It only bites below ~2 ms ticks on tiny networks,
i.e. below the real-time-controllable size anyway.

### 9. Multi-simulator coupling — NEST↔NEURON↔… with a shared clock?
This is MUSIC's advantage and **NCP does not do it**. MUSIC's reason to exist is
synchronizing *several* simulators on a global clock. NCP serves *one* NEST
instance; coupling two simulators on a cluster is a MUSIC job, not an NCP job.

### 10. "Stopping" semantics — does returning control between chunks count as stopping?
Between `Run` chunks, control returns to the client and the kernel is paused (not
advancing) until the next `Run`. But MUSIC also blocks the simulator at each tick
boundary for exchange — neither advances *during* the exchange. The distinction
that matters for the question ("without stopping like MUSIC") is **teardown vs
pause**: NCP never tears down/rebuilds the kernel (it pauses at a boundary and
resumes), exactly as MUSIC pauses at a tick and resumes. So NCP is "non-stopping"
in precisely the way MUSIC is.

## Final answer

NCP **can** get data from NEST in real time without stopping/rebuilding the
simulation. Mechanistically it is the NEST-native `Prepare`/`Run`/`Cleanup`
persistent-kernel loop with per-chunk delta readback and runtime stimulus
injection — functionally the same boundary-exchange contract as MUSIC, exposed
over a network/multi-language transport instead of MPI. With the streaming control
plane (`ncp_zenoh::ZenohControlTransport` + `ncp_core::NeuroControlLoop`), this
exchange flows continuously over the Zenoh action/perception planes — no per-tick
RPC. NCP is **not** a replacement for MUSIC's multi-simulator clock coupling; it is
the better choice when you need *one running NEST simulation served, live, to
remote/heterogeneous clients with QoS, safety, provenance, and a free analysis
tap*.

**Caveats / good practice.** Use the `NestController` `n_events`-delta readback for
long runs (the `NestSession` path slices a growing events array — O(history) per
step). Pick `chunk_ms` for your latency/throughput point as you would a MUSIC tick.
Per-exchange transport latency is network-bound but **small in absolute terms**
(≈0.1–1 ms) — below MUSIC's tick/buffering-bound *closed-loop* floor (§8), not above
it. See [`RATIONALE.md`](RATIONALE.md) §MUSIC for the full comparison.

**Use MUSIC, not NCP, when** you need to co-schedule **several simulators on one
shared clock** (e.g. NEST ↔ NEURON in a single MPI world, §9), or you need **bulk
intra-HPC spike throughput** (collective MPI moves spikes far faster than a networked
transport). **Use NCP, not MUSIC, when** you need **one** running NEST simulation
served *live* to **remote, heterogeneous, multi-language** clients with per-plane QoS,
a safety governor, provenance, and a free read-only observer tap — where a single
closed-loop reaction is, if anything, lower-latency than MUSIC's buffered tick.

---

## The chunking question: `calibrate()`, `min_delay`, and why this is not a tax MUSIC avoids

A recurring, reasonable worry: *NEST has a `calibrate()` step that runs in C++ every
time the simulation starts/resumes; MUSIC seems to run "continuously" without it, but
NCP runs in `Run(chunk)` slices — so doesn't NCP re-pay `calibrate()` every chunk,
making big networks slower and harder to code (delays, plasticity)?*

The short answer is **no, on all three counts** — and the reason is structural, not a
tuning trick. The detail matters, so here it is from first principles.

### 1. `calibrate()` is now `pre_run_hook()`, and it runs once per `Prepare()`, not per `Run()`

In NEST 3.x the per-node lifecycle hook formerly called `calibrate()` was renamed
**`pre_run_hook()`** (alongside `init_state()` / `init_buffers()`). It precomputes the
time-step-dependent constants a node needs — e.g. the exact integration propagators for
`iaf_psc_alpha` (`exp(-h/tau_m)` and friends), which depend on the resolution `h` and
the neuron's time constants.

The key fact is **where** it is called. NEST's stepwise API splits the lifecycle
deliberately:

| Call | What it does | Cost |
|---|---|---|
| `nest.Prepare()` | prepares the connection infrastructure and **calls `pre_run_hook()` on every node** (the calibration) | once |
| `nest.Run(t)` | advances the kernel by `t` — integrate + deliver spikes at `min_delay` boundaries | per chunk; **no node `pre_run_hook`** |
| `nest.Cleanup()` | tears the prepared state down | once |

This is verified in the **NEST 3.9 source**, not only inferred from the bench. The node
calibration lives in `nestkernel/node_manager.cpp`, `NodeManager::prepare_node_()` —
literally `n->init(); n->pre_run_hook();` — which is reached **only** from
`prepare_nodes()`, and `prepare_nodes()` is called **only** from
`SimulationManager::prepare()` (guarded by the "Prepare called twice." throw).
`SimulationManager::run()`, before it advances, calls **only** `io_manager.pre_run_hook()`
— a *different*, cheap hook that flushes recorder/IO-backend buffers — and **never**
`prepare_nodes()`. So a node's `pre_run_hook()` (the propagator recompute) fires exactly
once per `Prepare()`, no matter how many `Run()` chunks follow.

`nest.Simulate(t)` is *defined as* `Prepare(); Run(t); Cleanup()`. So the behaviour you
remember — "`calibrate()` ran every time I started/resumed" — is exactly what a
**`Simulate()`-per-chunk loop** does: it re-prepares (re-calibrates) and re-cleans-up on
every call. That is a real and well-known anti-pattern.

**NCP does not do that.** The reference backend (`backends.py`: `NestBackend.open()` →
`NestSession.step()` → `close()`) calls `nest.Prepare()` **once** at session open,
`nest.Run(chunk)` for each control tick, and `nest.Cleanup()` **once** at close. The
kernel stays "prepared" across every chunk; `pre_run_hook()` fires once, at open, and
never again for the life of the session. (This is the entire reason the
`Prepare`/`Run`/`Cleanup` API was introduced — to lift calibration out of the inner
loop for exactly the co-simulation / stepwise use case.)

This is not a claim on faith — it is **benchmarked**.
[`scripts/bench_chunk_overhead.py`](scripts/bench_chunk_overhead.py) times three
patterns on a real network and labels them:

- **monolithic** — `Prepare(); Run(T); Cleanup()` (one `Run`).
- **chunked-efficient** — `Prepare()` **once**, `Run(chunk)` in a loop, `Cleanup()` at
  the end. *"The only added cost is the per-`Run()` call overhead."* ← the NCP pattern.
- **chunked-naive** — `nest.Simulate(chunk)` per chunk = `Prepare()`+`Run()`+`Cleanup()`
  **every chunk** (re-calibration each time). *Shown to demonstrate the penalty NCP
  avoids.*

If `pre_run_hook()` ran per-`Run()`, "chunked-efficient" would collapse into
"chunked-naive" — they would cost the same. They do not; that gap **is** the
once-per-`Prepare` calibration, measured. You can see it directly on your own NEST with
[`scripts/verify_nest_chunking.py`](scripts/verify_nest_chunking.py): it counts NEST's
`"Preparing N node(s) for simulation."` log line (emitted by `prepare_nodes()`) and
finds `Prepare()` + 50×`Run()` logs it **once**, while `Simulate()`×50 logs it **50
times**.

### 2. MUSIC also chunks — it does not run NEST "continuously"

The premise that "MUSIC avoids this" doesn't hold. MUSIC is a *runtime coordinator*: at
each MUSIC **tick** it blocks every coupled simulator, exchanges data over MPI, then lets
them advance to the next tick. On the NEST side that is the **same** persistent-kernel
stepwise loop — `Prepare()` once, then advance one tick at a time, exchanging at the
boundary — i.e. NEST-under-MUSIC is itself a `Run(tick)` loop. MUSIC does not make NEST
skip `pre_run_hook` or run without tick boundaries; it *is* a tick loop with an MPI
exchange at each boundary.

Three independent confirmations from the NEST 3.9 source: (i) MUSIC port setup is part of
the **same once-per-`Prepare` calibration** — `SimulationManager::prepare()` carries the
comment *"we use calibrate to map the ports of MUSIC devices, which has to be done before
enter_runtime"* — so MUSIC gets no exemption from the prepare-once model; (ii) NEST
advances under MUSIC by calling `music_runtime->tick()` once per `min_delay` slice (the
NEST MUSIC tutorial's runtime loop: *simulate a slice → `tick()` to communicate →
repeat*); (iii) **every** NEST simulation — monolithic, chunked, or MUSIC — is internally
sliced into `min_delay` intervals anyway: `run()` clamps `to_step_ = std::min(from_step_ +
to_do_, get_min_delay())` and gathers/delivers spikes only at slice end. So both MUSIC and
NCP chunk at `min_delay`; NCP's `chunk_ms` is just a coarser, user-chosen multiple.

So NCP and MUSIC use the **identical** NEST execution model (calibrate once, advance in
slices, exchange at boundaries). They differ in **one** axis — the transport at the
boundary: MUSIC exchanges over **buffered pairwise MPI `Send`/`Recv`** within one
co-launched allocation (Djurfeldt et al., *Neuroinform.* 2010, PMC3240549); NCP uses
Zenoh / a localhost gateway / WebSocket to reach remote, heterogeneous, multi-language
clients (§8 above). That is a transport trade, not a recalibration tax — and, perhaps
counter-intuitively, **not even a closed-loop-latency loss for NCP** (see the latency
note in §8). There is **no per-chunk `calibrate()` that NCP pays and MUSIC escapes.**

### 3. Chunking does not change the science — synapse timing is chunk-invariant

Stopping to read between `Run(chunk)` calls cannot shift a spike or a delay. NEST holds
in-flight spikes in per-target **ring buffers** keyed by delivery time: a spike emitted
at `t` on a synapse of delay `d` is delivered at `t + d` regardless of whether a `Run()`
returned control at some boundary in between. Chunk boundaries are bookkeeping points for
*the host*, not events in *the model's* time. Heterogeneous delays, STDP, structural
plasticity — all evolve from the persistent kernel state and are blind to where you chose
to pause.

`bench_chunk_overhead.py` enforces this: with a fixed RNG seed it **asserts bit-identical
total spike counts** between monolithic and chunked-efficient (`--strict` exits non-zero
on any divergence). Same seed, same science, whatever the chunk size. "Different synapse
timing" is therefore not a correctness hazard of chunking — it is handled by the kernel,
not by the host loop. (The companion test below extends this to assert identical *spike
times* and *STDP weights*, not just counts.)

### 4. The one real constraint is `min_delay` — and it is shared with MUSIC

NEST exchanges spikes between threads/MPI ranks at intervals of **`min_delay`** (the
smallest synaptic delay in the network): within a `min_delay` window every node can
integrate independently, and spikes are communicated at the window boundary. This sets a
*natural* granularity: a `Run(chunk)` that is a small multiple of `min_delay` amortises
the per-`Run` communication/collocation work efficiently; chunking *far below* `min_delay`
just pays that fixed per-`Run` bookkeeping more often for no scientific benefit.

This is a **NEST property, identical under MUSIC** — a MUSIC tick is likewise typically
chosen at/above `min_delay`, and the ROS-MUSIC toolchain's own tick-latency trade
(≈70 ms at a 1 ms tick … ≈350 ms at a 50 ms tick, §5 above) *is* this same knob. Good
practice for NCP is the same as for MUSIC: **set `chunk_ms` to a small multiple of
`min_delay`** at the largest value your control latency tolerates.

### 5. Big simulations get *easier*, not harder, on this axis

The per-`Run()` overhead (buffer collocation + the `min_delay` spike exchange + entry/exit
bookkeeping) is **independent of network size for a fixed chunk** — it does not grow with
neuron/synapse count. The integration cost `T_run`, on the other hand, grows with the
network. So for a *large* network the fixed per-chunk overhead is a **vanishing fraction**
of `T_run`: the bigger the simulation, the less chunking costs you, relatively. The worry
inverts. (Where a big network *does* bite is the real-time factor — see "Real-time factor
& sizing" below — but that is set by model size and thread/MPI scale-out, and is
**identical** for MUSIC. Chunking adds nothing to it.)

### 6. So: no NEST-core change is needed — the workaround is the pattern already in use

Putting it together, the way to drive a large NEST network in slices without paying a
recalibration tax — **without modifying NEST core** — is exactly what the reference
backend does:

1. `nest.Prepare()` **once** per session (calibrates once); never `nest.Simulate()` in
   the loop.
2. `nest.Run(chunk)` per tick, with `chunk` an **integer multiple of `min_delay`**
   (read it from `nest.GetKernelStatus("min_delay")`). Prefer a *large* throughput chunk
   and drop to a finer one only when control latency demands it.
3. Drive stimuli between `Run`s without re-preparing. The reference backend mutates
   `dc_generator.amplitude` / `poisson_generator.rate` via `device.set()` between `Run`s
   (O(1) per node; does **not** invalidate the prepared state). This works, but NEST
   cautions that `SetStatus` *between* `Prepare()` and `Cleanup()` can "lead to
   unpredictable results"; the robust alternative is a **scheduled-time generator**
   (`step_current_generator` / `step_rate_generator`) whose next value is scheduled at
   `biological_time + dt` — exactly the template the backend already uses for the
   `rate_inject` path.
4. `nest.Cleanup()` **once** at session close.

> **Two implementer caveats (correctness, not speed).** (a) If `chunk_ms` is **not** an
> integer multiple of `min_delay` *and* the network has multiple RNG sources, NEST's own
> *"requested simulation time is not an integer multiple of the minimal delay"* applies
> and chunked vs monolithic results can legitimately diverge — snap `chunk_ms` to
> `min_delay` (step 2) to keep bit-identity. The reference backend does not yet snap, so
> a consumer choosing an arbitrary `chunk_ms` should. (b) Prefer scheduled-time
> generators (step 3) over live `device.set()` for stimulus where exact reproducibility
> matters.

For lower *exchange* latency the lever is the transport (MPI vs Zenoh), not the chunking —
and if you genuinely need a shared-clock coupling of *several* simulators, that is MUSIC's
job (§9), which NCP does not try to replace.

### 7. Verifying on NEST 3.9

The numbers in [`PERFORMANCE.md`](PERFORMANCE.md) and the
[`scripts/bench_*.py`](scripts) were measured on **NEST 3.8.0**; the reference backend
**requires NEST 3.9**. The execution model above (`Prepare`/`Run`/`Cleanup`, `pre_run_hook`
in `Prepare`, `min_delay` exchange, ring-buffer delay delivery) is unchanged across 3.8/3.9
— but because the empirical claims are version-specific, the repo ships a runnable check
you can execute against *your* NEST 3.9:

[`scripts/verify_nest_chunking.py`](scripts/verify_nest_chunking.py) proves, on the
installed NEST, that (a) the calibration/preparation cost is paid **once per `Prepare()`**,
not per `Run()` — operationally, by showing the `Prepare`-once chunked loop costs
≈ monolithic plus a small fixed per-`Run` overhead, while the `Simulate()`-per-chunk path
(which re-`Prepare`s, i.e. re-calibrates, every chunk) costs many times more; (b) that
fixed per-`Run()` overhead is small and chunk-count-driven (one `Run(T)` vs `N`×`Run(T/N)`);
and (c) chunked vs monolithic give **bit-identical spike times *and* STDP weights**, not
just counts. See its header for the exact commands.

---

## Real-time factor & sizing (measured)

§7 above says neither MUSIC nor NCP *guarantees* real time — that the network
size sets whether NEST keeps up with wall-clock. This section turns that into
numbers. The *real-time factor* `rt = bio_time / wall_time`: `rt >= 1.0` means the
kernel integrates faster than real time (the precondition for a live loop);
`rt < 1.0` means the loop lags and is only usable offline.

**The binding constraint is the real-time factor, not the chunk size.**
`chunk_ms` sets control-loop *latency* and is cheap to shrink — but it cannot make
a network that integrates at 0.3x real time keep up. Only fewer neurons, fewer
synapses-per-neuron, more threads, or MPI scale-out move the real-time factor.
(Shrinking the chunk while compute-bound makes it *worse* — per-`Run()` overhead
grows; see [`PERFORMANCE.md`](PERFORMANCE.md).)

### Method

Brunel-style balanced random network (the NEST standard scaling benchmark):
`iaf_psc_alpha`, 0.8N excitatory / 0.2N inhibitory, **fixed indegree** held
constant across N (`fixed_indegree`, CE=400 from E + CI=100 from I ⇒ ~500
recurrent synapses/neuron), inhibition-dominated (g=5), per-neuron Poisson drive
tuned for an async-irregular **~13 Hz** regime. Readback is a `spike_recorder` on
a 1000-neuron readout subset only (mimics an NCP `RecordSpec`; recording overhead
is negligible, so these are raw integrate + spike-delivery numbers). Build is
**outside** the timer; only `nest.Simulate(T_bio)` is timed; one untimed warmup,
then up to 3 timed reps with the **MIN wall** reported. NEST 3.8.0, OpenMP-only,
single MPI rank, 16 physical cores, 128 GB RAM. Reproduce with
[`scripts/bench_realtime.py`](scripts/bench_realtime.py).

The full benchmark methodology — this real-time sweep plus the chunk-overhead and
I/O-overlap/GIL benchmarks, with the timing protocol, correctness/equivalence
checks, exact commands, environment, and caveats (including the `conda run`
stdout-buffering caveat: run the env interpreter directly with `-u`) — is
documented once in
[`PERFORMANCE.md` → Benchmark methodology & reproducibility](PERFORMANCE.md#benchmark-methodology--reproducibility).
This section keeps the sizing results; see there for the method shared across all
three benchmarks.

### Real-time factor vs network size and threads

`rt` (bio-s per wall-s); **bold** = real time or faster.

| N (neurons) | ~synapses | T=1 | T=2 | T=4 | T=8 | T=16 |
|---|---|---|---|---|---|---|
| 10 000 | 5.0 M | 0.32 | 0.63 | **1.18** | **2.01** | **2.13** |
| 50 000 | 25 M | 0.033 | 0.063 | 0.14 | 0.30 | 0.35 |
| 100 000 | 50 M | 0.014 | 0.032 | 0.066 | 0.13 | 0.17 |
| 200 000 | 100 M | 0.0065 | 0.013 | 0.027 | 0.054 | 0.071 |

(N=200000 used T_bio=500 ms; `rt` scaled to its own bio time. N=100000 T=1 and
N=200000 T=1 ran a single timed rep, having exceeded a 60 s/rep skip threshold.)

### The real-time frontier — largest live-controllable network

* **Real time (>=1x) is reached ONLY at N=10000, and only at >=4 threads.** Within
  N=10000 the crossing sits between T=2 (0.63x) and T=4 (1.18x): ~3–4 threads are
  needed to drive a 10k-neuron / 5M-synapse net at ~13 Hz in real time.
* **No N>=50000 config reaches real time on 16 cores.** The closest is N=50000 at
  T=16 = 0.35x (~2.85x slower than real time).
* **Practical live ceiling at 16 threads / ~13 Hz / ~500 syn/neuron: ~10k–20k
  neurons (~5–10M synapses).** The ~17k–20k crossing at T=16 is an *interpolation*
  (the sweep did not sample between 10k and 50k), not a measured point.

Because indegree is fixed, total synapses and per-step compute scale ~linearly
with N — which is why `rt` degrades roughly linearly with N at fixed threads.
Firing stayed 12.3–13.5 Hz across every (N,T) cell, confirming the regime is
N-invariant.

### Thread-scaling efficiency

Efficiency = speedup(T) / T relative to the same-N T=1 baseline.

| N | T=2 | T=4 | T=8 | T=16 |
|---|---|---|---|---|
| 50 000 | 1.89x (0.95) | 4.09x (**1.02**) | 8.98x (**1.12**) | 10.51x (0.66) |
| 10 000 | 1.94x (0.97) | 3.64x (0.91) | 6.22x (0.78) | 6.60x (0.41) |

* **Super-linear speedup at T=4/T=8 is real and reproducible** (efficiency >100%):
  at 16 physical cores the per-thread neuron/synapse slice shrinks enough to fit
  cache better, so adding threads more than proportionally helps up to ~8.
* **Efficiency collapses at T=16** (0.66 on the 50k net, 0.41 on the 10k net): all
  physical cores saturate, and memory-bandwidth + spike-delivery/event-buffer
  synchronization dominate. 16 threads still cuts absolute wall time, but with
  ~65–70% efficiency on big nets and worse on small ones. **Do not extrapolate
  linear scaling past 8 threads.** Best parallel efficiency lives in the 4–8 thread
  band; the small 10k net saturates earliest (too little work per thread to amortize
  16-way overhead).

### What was NOT the limiter

* **Memory:** RSS peaked ~5 GB at N=200000 / 100M synapses, far under 128 GB.
* **Build time** was the *emerging* limiter at large N (not RAM): build grew
  0.34 s (5M syn) → 7.2 s (50M) → 14.9 s (100M) at T=1, ~linear in synapse count.
  It is outside the timer, but at N=200000 the one-time build is a real fraction of
  any short run.
* **Compute/wall time** was the binding constraint for real time.

### Concrete guidance

1. **Pick threads in the 4–8 band first.** That is where efficiency is highest
   (often >=100%). Going to all 16 cores helps absolute wall time but with
   diminishing returns; it is not free headroom.
2. **Size the brain to the budget.** For a >=1x live loop on 16 cores at ~13 Hz with
   ~500 syn/neuron, stay at <=~10k–20k neurons / <=~5–10M synapses. A bigger live
   brain needs MPI scale-out, lower indegree, or accepting sub-real-time.
3. **Accept non-real-time for offline.** If the science needs 50k+ neurons and the
   loop need not be live, `rt < 1` is fine — just do not advertise the session as
   real time (the roadmap's open-session budget check makes this honest, not silent).
4. **Shrink the chunk for latency, not for throughput.** `chunk_ms` trades latency
   against per-`Run()` overhead; it does not change the real-time factor.

### Caveats

* Numbers are specific to **NEST 3.8.0 (OpenMP-only, single MPI rank, 16 cores)**,
  this connectivity (~500 recurrent syn/neuron via fixed indegree), and this firing
  regime (~13 Hz async-irregular). CLAUDE.md pins NESTML 8.2.0 → NEST 3.9 as the
  target; numbers may shift slightly on 3.9.
* The ~17k–20k live ceiling at T=16 is interpolated (no sample between 10k and 50k).
* `fire_hz` is reported from the first-rep event count, not the min-wall rep
  (harmless — the rate is N/T-invariant by design).
* Independently re-verified at the representative N=50000 cell (T=1 ~32 s/s vs 30
  reported, T=8 ~3.74 vs 3.34 — within ~7–12%, no trend reversal); the rest of the
  grid is a faithful transcription of the recorded run.
