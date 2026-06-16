# Does NCP bottleneck NEST? — performance review

**Short answer: no, not inherently — once one real bottleneck (now fixed) is out
of the way.** NEST only advances simulation time *during* `nest.Run(chunk)`;
between chunks NCP does its work (read recorders, serialize, transport, inject
stimulus). So the effective throughput is

```
effective_rate ≈ chunk_ms / (T_run + T_ncp)
```

NCP is a bottleneck only if its per-chunk overhead `T_ncp` is comparable to or
larger than the integration time `T_run`. For a **rate-coded control loop** `T_ncp`
is small and bounded; the dominant term is `T_run` — the simulation itself, which
NCP neither can nor should change.

## The one real bottleneck — found and fixed

`NestSession.step` previously called `device.get("events")` for **every** record
port on **every** step and sliced `[last:]` (`backends.py`). `get("events")`
materialises the recorder's **entire history** each call, so per-step cost grew
**O(total events recorded)** — linearly with run length. A 10-minute control loop
at 50 Hz would slow to a crawl (and balloon memory) purely from re-copying spike
history, even though each step only needs the last chunk. This *would* have
throttled NEST as the loop fell behind real time.

Fixed (`backends.py::NestSession.step`):
- **`RATE`** (the common control observable): difference the **`n_events`
  counter** — **O(1)** per step, no events array at all (the proven
  `loop.py::NestController` pattern).
- **`SPIKES` / `V_m`**: fetch the events, return the `[last:]` tail, then
  **best-effort drain** the recorder (`set(n_events=0)`) so the next read is
  **O(new)**; if the NEST build doesn't support clearing, fall back to index
  tracking (correct, but the array keeps growing — prefer `RATE` for long loops).

Result: per-step readback is **O(1)** for rate and **O(new events)** for spikes/V_m
— bounded, independent of run length.

## Per-tick cost model (after the fix)

| Term | Cost | Notes |
|---|---|---|
| stimulus inject (`generator.set`) | O(#stimulus ports), µs | a few `set()` calls |
| **`nest.Run(chunk)`** | **dominant; model-size dependent** | the science; NCP doesn't touch it |
| readback | **O(1)** rate / **O(new)** spikes-V_m | was O(history) — fixed |
| encode/decode (codec) | negligible | linear rate map |
| serialize | rates: tens of bytes; raw spikes: O(events) | **prefer rate for the loop**; raw spikes are the analysis path |
| transport | in-proc ~µs · Zenoh SHM ~tens µs · Zenoh/loopback ~0.1 ms · WS+JSON ~0.2–1 ms | far below a 20–50 Hz (20–50 ms) budget |

For a UAV outer-loop (20–50 Hz) with rate-coded I/O, `T_ncp` is sub-millisecond
and **`T_run` dominates**. NCP adds no meaningful slowdown.

## Secondary considerations (not bottlenecks, documented)

- **Hot loop bypasses the gateway.** The streaming control plane is
  `ZenohControlTransport` pub/sub (sensor→command); it does **not** go through the
  gateway's per-request RPC. The gateway's localhost-TCP-per-request is only the
  *rare* lifecycle RPC (open/close); it is not on the per-tick path. (Connection
  reuse there is a possible micro-optimisation, not a bottleneck.)
- **WebSocket single-thread executor.** `backend/api/neurocontrol.py` runs
  `handle_json` on one shared worker thread — this serialises all connections'
  NEST work, which is *correct* (one global NEST kernel) and keeps the event loop
  free; it is not an added bottleneck (NEST is single-kernel regardless).
- **Raw spike streaming** at high rate produces large JSON. Use `RATE`/counts for
  the control loop; stream raw spikes only on the observation/analysis plane
  (an analysis/observer client), which is loss-tolerant.

## Compared with SOTA (June 2026)

Runtime exchange with a live simulator has *inherent* per-tick overhead in every
scheme — MUSIC services its ports each MPI tick, NRP-core marshals DataPacks per
step, the NEST Server does a REST round trip. NCP's chunked `Prepare`/`Run` +
**delta readback** is the standard pattern, and after this fix its readback is the
same **O(new)** MUSIC achieves. The one structural difference is the transport
hop: MUSIC uses MPI shared memory (lowest latency, same allocation); NCP uses a
network/IPC transport (slightly higher, but ≪ the control-rate budget — see
[`NEST_REALTIME.md`](NEST_REALTIME.md)) in exchange for remote, multi-language,
fleet reach. NCP does **not** claim to beat MPI on raw latency; it competes on
portability, safety, provenance and observability.

## Measured: chunk overhead, scaling, and I/O overlap (NEST 3.8.0, 16 cores)

The cost model above predicts `T_ncp` is small and `T_run` dominates. Three
benchmarks confirm it and bound where NCP's design choices actually matter.
Reproduce with [`scripts/bench_realtime.py`](scripts/bench_realtime.py) and
[`scripts/bench_overlap.py`](scripts/bench_overlap.py); full sizing table in
[`NEST_REALTIME.md`](NEST_REALTIME.md).

### Per-chunk readback overhead — already ~free

The earlier readback fix (above) made per-step readback **O(1)** for rate and
**O(new events)** for spikes/V_m. The real-time sweep recorded from a 1000-neuron
readout subset (an NCP `RecordSpec`) and saw recording overhead stay negligible —
the measured numbers are raw integrate + spike-delivery throughput, not dominated
by readback. The control-observable path is not the bottleneck; `T_run` is.

### Scaling: the binding constraint is the real-time factor

A Brunel-style balanced net (~500 syn/neuron, ~13 Hz async-irregular) reaches
**>=1x real time only at N=10000 and only at >=4 threads** (T=4 1.18x, T=8 2.01x,
T=16 2.13x). No N>=50000 config reaches real time on 16 cores (best N=50000 T=16 =
0.35x). Since indegree is fixed, synapses and per-step compute scale ~linearly with
N, so `rt` degrades ~linearly with N. Thread efficiency peaks in the **4–8 band**
(super-linear, cache-driven: N=50000 T=8 efficiency ~1.12) and collapses to ~0.66
at T=16 — 16 threads still helps absolute wall time but with diminishing returns.
**Practical live ceiling at 16 threads / ~13 Hz / ~500 syn/neuron: ~10k–20k
neurons.** Implication for `chunk_ms`: shrinking it buys latency, not throughput,
and while compute-bound it makes things *worse* (per-`Run()` overhead climbs — at a
10 ms chunk on a 50k net, ~10 ms of bio time cost ~38–65 ms of compute). If real
time at large N is the goal, the lever is **fewer-but-larger chunks / more threads /
a smaller net**, not a smaller chunk.

### I/O overlap: in-process Python threading CANNOT overlap transport with compute

A decisive GIL test settles where transport must live: a background spinner thread
retained only **~0.4–1.3% of its standalone counting rate during a real
`nest.Run()`** (a released GIL would keep >50%). **`nest.Run()` holds the Python
GIL for its full duration** (`gil_released=false`). Consequence, measured: a
`ThreadPoolExecutor` "overlap" loop delivered **0.92–1.10x speedup across all cases
— i.e. noise** — even when modeled transport I/O (5 ms) was comparable to per-chunk
compute (~4.5 ms), because the background thread cannot serialize while `Run` owns
the GIL. Overlap only pays off when per-chunk I/O is comparable-to-greater-than
per-chunk compute **AND** transport runs outside the GIL; in-process threading fails
the second condition. **Therefore transport must not live in the NEST interpreter:
put it in the Rust NCP gateway ([`ncp-gateway`](ncp-gateway) / [`ncp-zenoh`](ncp-zenoh)),
whose OS threads run fully outside the GIL** and can ship chunk N-1 / buffer chunk
N+1 while the NEST process computes chunk N. For compute-bound heavy nets, overlap
is pointless regardless (best honest case stayed at a ~55 ms period at chunk_ms=10).

(Overlap caveat: the original `bench_overlap.py` prototype was deleted after the
first run and reconstructed; the reconstruction reproduces the qualitative verdicts
— GIL held, threaded overlap ~1.0x — but absolute per-chunk-compute magnitudes
differ by >2x because the exact original Poisson drive was unknown. The load-bearing
finding, not the absolute periods, is what reproduces.)

## What to measure on your hardware

1. `T_run` for your network at your `chunk_ms` — this sets the feasible rate.
2. Readback cost (now O(1)/O(new)) and end-to-end tick time.
3. **p99 jitter**, not mean — the thing a control loop actually cares about.
4. Then pick `chunk_ms` for your latency/throughput point (as you would a MUSIC tick).

## Honest remaining items

- The spikes/V_m **drain is best-effort** — confirm `set(n_events=0)` clears on
  your NEST 3.9 build; otherwise that path stays O(history) (use `RATE`).
- Large-population multimeter recording is intrinsically heavy regardless of NCP;
  record from a representative subset (the backend already pins V_m to one neuron).
