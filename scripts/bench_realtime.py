#!/usr/bin/env python3
"""Real-time-factor sweep for a NEST spiking network served over NCP.

WHAT THIS MEASURES
------------------
The *real-time factor* of a NEST simulation: how many seconds of biological
time the kernel integrates per second of wall-clock. ``rt_factor >= 1.0`` means
the network can be driven faster than real time (the precondition for a live,
NCP-served control loop); ``rt_factor < 1.0`` means the loop will lag wall-clock
and is only usable offline.

This is the binding constraint for live control. ``chunk_ms`` (the NCP control
exchange granularity) sets loop *latency* and is cheap to shrink, but it cannot
make a network that integrates at 0.3x real time keep up — only fewer neurons /
fewer synapses-per-neuron / more threads / MPI scale-out can.

NETWORK
-------
Brunel-style balanced random network (the NEST standard scaling benchmark):
``iaf_psc_alpha``, N split NE=0.8N excitatory / NI=0.2N inhibitory. Indegree is
held FIXED across N (``fixed_indegree``: CE from E, CI=CE/4 from I), so total
synapse count scales ~linearly with N and so does per-step compute. Inhibition
dominated (g=5), one ``poisson_generator`` per neuron tuned for an
async-irregular ~13 Hz regime. A ``spike_recorder`` reads back only a small
readout subset (mimicking an NCP ``RecordSpec``; keeps recording overhead
negligible so the numbers reflect raw integrate + spike-delivery throughput).

METHODOLOGY (matters for honest numbers)
----------------------------------------
* ``local_num_threads`` is set BEFORE node creation (required by NEST).
* Network build is done OUTSIDE the timer; only ``nest.Simulate(T_bio)`` is timed.
* One untimed warmup, then up to ``--reps`` timed reps; the MIN wall is reported.
* ``rt_factor = (T_bio_ms / 1000) / min_wall_s``.

REQUIRES NEST
-------------
Needs a working NEST install (``import nest``). Benchmarked on NEST 3.8.0
(OpenMP-only, single MPI rank). Numbers shift on other versions / hardware.
Run the env interpreter DIRECTLY (e.g. ``/opt/anaconda3/envs/p2b/bin/python -u
bench_realtime.py``) rather than ``conda run`` — ``conda run`` fully buffers
child stdout when redirected, so per-row progress never streams.

EXAMPLE
-------
    python bench_realtime.py --n 10000 50000 --threads 1 4 8 16 \
        --t-bio-ms 1000 --reps 3

REFERENCE NUMBERS (NEST 3.8.0, 16 physical cores, ~500 syn/neuron, ~13 Hz)
--------------------------------------------------------------------------
Real-time (>=1x) reached ONLY at N=10000 and only at >=4 threads
(T=4 1.18x, T=8 2.01x, T=16 2.13x). No N>=50000 config reaches real time on
16 cores (best N=50000 T=16 = 0.35x). Practical live ceiling at 16 threads /
~13 Hz / ~500 syn/neuron: ~10k-20k neurons (~5-10M synapses). See NEST_REALTIME.md.
"""
from __future__ import annotations

import argparse
import time


def build_brunel(n: int, threads: int, indegree_ce: int, poisson_hz: float,
                 seed: int, dt_ms: float, readout: int):
    """Build (outside the timer) a Brunel-style balanced random network.

    Returns the spike_recorder gid and the number of recurrent synapses.
    """
    import nest

    nest.ResetKernel()
    # Threads MUST be set before any Create().
    nest.local_num_threads = threads
    nest.resolution = dt_ms
    nest.rng_seed = seed

    ne = int(0.8 * n)
    ni = n - ne
    ce = indegree_ce          # excitatory indegree (fixed across N)
    ci = ce // 4              # inhibitory indegree
    delay_ms = 1.5
    j_exc = 20.0              # pA
    g = 5.0                   # inhibition-dominated
    j_inh = -g * j_exc

    exc = nest.Create("iaf_psc_alpha", ne)
    inh = nest.Create("iaf_psc_alpha", ni)
    pg = nest.Create("poisson_generator", params={"rate": poisson_hz})
    rec = nest.Create("spike_recorder")

    nest.Connect(pg, exc + inh, syn_spec={"weight": j_exc, "delay": delay_ms})
    nest.Connect(exc, exc + inh,
                 conn_spec={"rule": "fixed_indegree", "indegree": ce},
                 syn_spec={"weight": j_exc, "delay": delay_ms})
    nest.Connect(inh, exc + inh,
                 conn_spec={"rule": "fixed_indegree", "indegree": ci},
                 syn_spec={"weight": j_inh, "delay": delay_ms})

    readout_n = min(readout, n)
    nest.Connect((exc + inh)[:readout_n], rec)

    n_syn = n * (ce + ci) + readout_n
    return rec, n_syn, readout_n


def sweep(args) -> dict:
    import nest

    grid = []
    for n in args.n:
        for threads in args.threads:
            t_bio = args.t_bio_ms
            # Optionally shorten very large nets to keep reps under budget.
            if args.shorten_above and n >= args.shorten_above:
                t_bio = min(t_bio, args.shorten_to_ms)

            build_t0 = time.perf_counter()
            rec, n_syn, readout_n = build_brunel(
                n, threads, args.indegree, args.poisson_hz,
                args.seed, args.dt_ms, args.readout)
            build_s = time.perf_counter() - build_t0

            # Untimed warmup.
            nest.Simulate(args.warmup_ms)
            n0 = rec.n_events

            walls = []
            skipped = False
            for _ in range(args.reps):
                t0 = time.perf_counter()
                nest.Simulate(t_bio)
                walls.append(time.perf_counter() - t0)
                if walls[-1] > args.skip_threshold_s:
                    skipped = True
                    break  # one timed rep is enough; don't burn budget

            n_ev = rec.n_events - n0
            fire_hz = (n_ev / readout_n) / (t_bio / 1000.0)
            min_wall = min(walls)
            rt_factor = (t_bio / 1000.0) / min_wall

            row = {
                "n": n, "threads": threads, "t_bio_ms": t_bio,
                "wall_s": round(min_wall, 4),
                "realtime_factor": round(rt_factor, 4),
                "build_s": round(build_s, 4),
                "n_syn": n_syn, "fire_hz": round(fire_hz, 2),
                "skipped": skipped,
            }
            grid.append(row)
            print("RTROW", row, flush=True)

    return {"nest_version": nest.__version__, "grid": grid}


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--n", type=int, nargs="+", default=[10000, 50000],
                   help="network sizes to sweep")
    p.add_argument("--threads", type=int, nargs="+", default=[1, 4, 8, 16],
                   help="local_num_threads values to sweep")
    p.add_argument("--t-bio-ms", type=float, default=1000.0,
                   help="biological time integrated per timed rep")
    p.add_argument("--reps", type=int, default=3, help="timed reps (MIN reported)")
    p.add_argument("--warmup-ms", type=float, default=200.0)
    p.add_argument("--indegree", type=int, default=400,
                   help="excitatory indegree CE (fixed across N); CI=CE/4")
    p.add_argument("--poisson-hz", type=float, default=2800.0,
                   help="per-neuron external Poisson rate (tuned for ~13 Hz)")
    p.add_argument("--readout", type=int, default=1000,
                   help="spike_recorder readout subset size (NCP RecordSpec)")
    p.add_argument("--seed", type=int, default=12345)
    p.add_argument("--dt-ms", type=float, default=0.1)
    p.add_argument("--skip-threshold-s", type=float, default=60.0,
                   help="if a rep exceeds this wall time, stop after 1 timed rep")
    p.add_argument("--shorten-above", type=int, default=200000,
                   help="for N >= this, shorten T_bio (rt_factor scaled to its own bio time)")
    p.add_argument("--shorten-to-ms", type=float, default=500.0)
    args = p.parse_args()

    try:
        import nest  # noqa: F401
    except ImportError:
        raise SystemExit(
            "This benchmark REQUIRES NEST (import nest failed). "
            "Install NEST (benchmarked on 3.8.0) and run the env interpreter "
            "directly, e.g. /opt/anaconda3/envs/p2b/bin/python -u bench_realtime.py")

    import json
    print(json.dumps(sweep(args), indent=2))


if __name__ == "__main__":
    main()
