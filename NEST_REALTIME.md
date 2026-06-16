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
