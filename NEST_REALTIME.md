# Can NCP read NEST in real time, without stopping the simulation (like MUSIC)?

**Short answer: yes.** NCP exchanges data with a *running* NEST kernel without
tearing it down or rebuilding the network — using NEST's own
`Prepare()` → `Run(chunk)` → `Cleanup()` stepwise model with a persistent kernel.
That is the same **boundary-exchange** model MUSIC uses; the difference from MUSIC
is the *deployment shape* (MUSIC co-schedules multiple simulators in one MPI world
with a shared clock; NCP serves one NEST instance to remote, multi-language
clients), **not** the ability to interact with a live simulation.

This is checked 10 ways below against the actual code
(`backend/neurocontrol/backends.py`, `backend/neurocontrol/loop.py`).

### 1. NEST execution model — does NCP use stepwise `Run`, or `Simulate`-and-stop?
`NestBackend.open()` calls `nest.Prepare()` **once**; `NestSession.step()` calls
`nest.Run(advance_ms)`; `close()` calls `nest.Cleanup()` (`backends.py:301,223,252`).
This is NEST's documented stepwise/continuous-interaction mode — the same mode the
NEST Server and co-simulation frameworks use — not a `Simulate()`-teardown loop.

### 2. Kernel-state persistence — is the network rebuilt between reads?
`ResetKernel()` happens **only at `open()`** (`backends.py:317`). Between `step()`s
there is no reset and no re-`Create`; the populations, recorders, generators and
all neuron state persist. Simulation time advances monotonically across chunks
(`sim_time_ms += advance_ms`). So "without stopping the simulation" holds in the
sense that matters: no teardown, no rebuild, continuous state.

### 3. Data readback — streaming deltas or re-reading history?
`NestSession.step()` reads each recorder's events and slices `[last:]`, returning
**only the events since the previous step** (`backends.py:229–242`) — a streaming
delta per chunk. `loop.py::NestController` goes further and reads `n_events`
counts, computing rate from the **count delta** (O(1), no event array)
(`loop.py:137–141`). Either way you get new data each tick from the live kernel.

### 4. Runtime input injection — can you stimulate mid-simulation?
Yes. Each `step()` sets generator parameters (`dc_generator.amplitude`,
`poisson_generator.rate`) **before** the `Run` chunk (`backends.py:213–222`), so a
controller drives the running network in real time — the inverse of MUSIC's input
event ports.

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
`NeuroControlLoop` sleeps to hold a target `rate_hz` (`loop.py:261–272`), pacing to
wall-clock when NEST is faster than real time. For large networks NEST may run
*slower* than real time — a property of the model size, identical for MUSIC (and
why neuromorphic on-chip wins on raw loop latency). Neither MUSIC nor NCP
*guarantees* real time; both provide *runtime* exchange.

### 8. Transport & reach — local only, or networked/multi-language?
MUSIC's runtime exchange is **MPI**, within one co-launched (possibly multi-node)
allocation, C++/Python. NCP carries the same per-chunk exchange over **Zenoh / a
localhost gateway / WebSocket**, to **remote, heterogeneous, multi-language**
clients (Rust/Python/TS/C++). This is NCP's advantage for the "serve one NEST sim
to a fleet + observers" case; it is also a higher per-exchange latency than MPI
shared memory.

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
Per-exchange latency is network-bound, above MPI shared memory. See
[`RATIONALE.md`](RATIONALE.md) §MUSIC for the full comparison.

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
