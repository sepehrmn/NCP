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
