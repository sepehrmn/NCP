#!/usr/bin/env python3
"""Can in-process Python threading overlap NCP transport I/O with NEST compute?

WHAT THIS MEASURES
------------------
Two things, in order:

1. **GIL test (decisive).** A background "spinner" thread increments a counter
   in a tight Python loop while a real ``nest.Run()`` advances the network. If
   ``nest.Run()`` RELEASED the GIL, the spinner would retain a large fraction of
   its standalone counting rate during Run. If it HOLDS the GIL, the spinner is
   starved (~0% of baseline). The measured verdict on NEST 3.8.0 is:
   **nest.Run() HOLDS the GIL for its full duration.**

2. **Overlap loop.** A chunked ``Run`` loop, run two ways with the SAME total
   work (same chunk count, same per-chunk serialize-I/O): (a) serial
   ``serialize_io(); Run()`` and (b) a ``ThreadPoolExecutor`` that tries to
   overlap ``serialize_io(chunk N)`` with ``Run(chunk N+1)``. Speedup ~1.0x means
   no real overlap. Because of the GIL verdict, in-process threading CANNOT
   overlap — the background thread cannot serialize while Run holds the GIL.

WHY THIS MATTERS FOR NCP
------------------------
Overlap only pays off when per-chunk transport I/O is comparable to or larger
than per-chunk compute (the I/O-bound regime). Even there, in-process Python
threading fails because Run holds the GIL. The architectural consequence: NCP's
transport stack (CommandFrame / SensorFrame serialization + Zenoh RTT) must NOT
live in the same Python interpreter as the NEST kernel if you want true overlap.
Put it in a SEPARATE PROCESS — the Rust NCP gateway (ncp-gateway / ncp-zenoh)
runs transport on its own OS threads, fully outside the GIL, so it can ship
chunk N-1 and buffer chunk N+1 while the NEST process computes chunk N. For the
compute-bound regime (heavy nets), overlap is pointless regardless: the lever is
fewer-but-larger chunks / more threads / a smaller net, not I/O overlap.

REQUIRES NEST
-------------
Needs a working NEST install (``import nest``). Benchmarked on NEST 3.8.0.
Run the env interpreter DIRECTLY (not ``conda run``) so stdout streams.

EXAMPLE
-------
    python bench_overlap.py --n 5000 --threads 16 --chunk-ms 10 \
        --t-bio-ms 1000 --io-ms 0.5 2 5
"""
from __future__ import annotations

import argparse
import json
import threading
import time
from concurrent.futures import ThreadPoolExecutor


def build_net(n: int, threads: int, indegree_ce: int, seed: int, readout: int):
    """Brunel-style iaf_psc_delta net, Prepare()'d, ready for chunked Run()."""
    import nest

    nest.ResetKernel()
    nest.local_num_threads = threads
    nest.rng_seed = seed

    ne = int(0.8 * n)
    ni = n - ne
    ce = indegree_ce
    ci = ce // 4
    exc = nest.Create("iaf_psc_delta", ne)
    inh = nest.Create("iaf_psc_delta", ni)
    pg = nest.Create("poisson_generator", params={"rate": 20000.0})
    rec = nest.Create("spike_recorder")

    nest.Connect(pg, exc + inh, syn_spec={"weight": 20.0, "delay": 1.5})
    nest.Connect(exc, exc + inh,
                 conn_spec={"rule": "fixed_indegree", "indegree": ce},
                 syn_spec={"weight": 20.0, "delay": 1.5})
    nest.Connect(inh, exc + inh,
                 conn_spec={"rule": "fixed_indegree", "indegree": ci},
                 syn_spec={"weight": -5.0 * 20.0, "delay": 1.5})
    nest.Connect((exc + inh)[: min(readout, n)], rec)

    nest.Prepare()
    return rec


def gil_test(rec, run_ms: float = 1000.0) -> dict:
    """Decisive GIL test: does the spinner keep counting during nest.Run()?"""
    import nest

    # Baseline standalone counting rate.
    stop = threading.Event()
    counter = {"n": 0}

    def spin():
        while not stop.is_set():
            counter["n"] += 1

    t = threading.Thread(target=spin)
    t.start()
    time.sleep(0.3)
    base = counter["n"] / 0.3
    stop.set()
    t.join()

    # Counting rate DURING a real Run().
    stop = threading.Event()
    counter["n"] = 0
    t = threading.Thread(target=spin)
    t.start()
    t0 = time.perf_counter()
    nest.Run(run_ms)
    dur = time.perf_counter() - t0
    during = counter["n"] / dur
    stop.set()
    t.join()

    retained = during / base if base else 0.0
    return {
        "baseline_per_s": round(base, 1),
        "during_run_per_s": round(during, 1),
        "retained_fraction": round(retained, 4),
        # A released GIL would retain >50%; HOLD shows ~0%.
        "gil_released": retained > 0.5,
    }


def serialize_io(io_ms: float) -> None:
    """One chunk of NCP transport work: real CommandFrame/SensorFrame round-trip
    JSON encode+decode PLUS a modeled transport RTT (time.sleep)."""
    cmd = {"kind": "command_frame", "seq": 1, "channels":
           {"rotor": {"data": [0.1, 0.2, 0.3, 0.4], "unit": None}}}
    sens = {"kind": "sensor_frame", "seq": 1, "channels":
            {"imu": {"data": [0.0] * 6, "unit": None}}}
    json.loads(json.dumps(cmd))
    json.loads(json.dumps(sens))
    if io_ms > 0:
        time.sleep(io_ms / 1000.0)


def run_loop(rec, chunk_ms: float, n_chunks: int, io_ms: float,
             overlapped: bool) -> float:
    """Return mean chunk PERIOD (ms). Serial = io then Run; overlapped = Run
    while a worker serializes the previous chunk (cannot truly overlap under GIL)."""
    import nest

    t0 = time.perf_counter()
    if not overlapped:
        for _ in range(n_chunks):
            serialize_io(io_ms)
            nest.Run(chunk_ms)
    else:
        with ThreadPoolExecutor(max_workers=1) as ex:
            fut = ex.submit(serialize_io, io_ms)
            for _ in range(n_chunks):
                nest.Run(chunk_ms)          # holds GIL
                fut.result()                # join prev serialize
                fut = ex.submit(serialize_io, io_ms)
            fut.result()
    total = time.perf_counter() - t0
    return (total / n_chunks) * 1000.0


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--n", type=int, default=5000, help="network size")
    p.add_argument("--threads", type=int, default=16)
    p.add_argument("--indegree", type=int, default=100,
                   help="excitatory indegree CE; CI=CE/4")
    p.add_argument("--chunk-ms", type=float, default=10.0)
    p.add_argument("--t-bio-ms", type=float, default=1000.0)
    p.add_argument("--io-ms", type=float, nargs="+", default=[0.5, 2.0, 5.0],
                   help="modeled per-chunk transport RTT values to sweep")
    p.add_argument("--readout", type=int, default=1000)
    p.add_argument("--seed", type=int, default=12345)
    p.add_argument("--out", type=str, default=None,
                   help="write JSON results to this file (creates parent dirs)")
    args = p.parse_args()

    try:
        import nest  # noqa: F401
    except ImportError:
        raise SystemExit(
            "This benchmark REQUIRES NEST (import nest failed). "
            "Install NEST (benchmarked on 3.8.0) and run the env interpreter "
            "directly, e.g. /opt/anaconda3/envs/engram/bin/python -u bench_overlap.py")
    import nest

    rec = build_net(args.n, args.threads, args.indegree, args.seed, args.readout)

    gil = gil_test(rec)
    print("GIL", json.dumps(gil), flush=True)

    n_chunks = int(args.t_bio_ms / args.chunk_ms)
    results = []
    for io_ms in args.io_ms:
        serial = run_loop(rec, args.chunk_ms, n_chunks, io_ms, overlapped=False)
        over = run_loop(rec, args.chunk_ms, n_chunks, io_ms, overlapped=True)
        row = {
            "io_ms": io_ms,
            "serial_period_ms": round(serial, 3),
            "overlapped_period_ms": round(over, 3),
            "speedup": round(serial / over, 3) if over else 0.0,
            "realtime_capable": serial <= args.chunk_ms,
        }
        results.append(row)
        print("OVERLAP", json.dumps(row), flush=True)

    nest.Cleanup()
    report = {"nest_version": nest.__version__, "gil": gil, "results": results}
    print(json.dumps(report, indent=2))

    if args.out:
        import os
        os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
        with open(args.out, "w") as f:
            json.dump(report, f, indent=2)
        print(f"Wrote results to {args.out}", flush=True)


if __name__ == "__main__":
    main()
