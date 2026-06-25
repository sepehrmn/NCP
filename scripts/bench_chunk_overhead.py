#!/usr/bin/env python3
"""How much does CHUNKED simulation cost vs MONOLITHIC, and is it equivalent?

WHAT THIS MEASURES
------------------
The per-chunk overhead of NCP's stepwise control model. NCP advances NEST in
``chunk_ms`` slices (``Prepare()`` once, ``Run(chunk)`` per control tick,
``Cleanup()`` at the end) so it can read recorders and inject stimulus between
chunks. This script quantifies how much that chunking costs versus a single
monolithic ``Run(T_bio)``, across a sweep of chunk sizes, and three ways:

1. **monolithic**          — ``Prepare(); Run(T_bio); Cleanup()`` (one Run call).
2. **chunked-efficient**   — ``Prepare()`` ONCE, then ``Run(chunk)`` in a loop,
                             ``Cleanup()`` at the end. This is the NCP pattern:
                             the kernel state persists across chunks, so the only
                             added cost is the per-``Run()`` call overhead.
3. **chunked-naive**       — the ANTI-PATTERN: ``nest.Simulate(chunk)`` per chunk,
                             which does ``Prepare()`` + ``Run()`` + ``Cleanup()``
                             internally EVERY chunk. Shown to demonstrate the
                             penalty NCP avoids by keeping the kernel prepared.

EQUIVALENCE (correctness, not just speed)
-----------------------------------------
Chunking must not change the science. Because the kernel state persists across
``Run(chunk)`` calls, monolithic and chunked-efficient must produce
**bit-identical total spike counts** for the same fixed RNG seed. The script
ASSERTS this (``--strict`` exits non-zero on any divergence). The naive path is
NOT required to match — each ``Simulate`` resets/teardowns, so it is reported for
timing only and excluded from the equivalence set.

NETWORK
-------
``iaf_psc_alpha``, NE excitatory + NI inhibitory neurons. Recurrent connectivity
is SPARSE (``fixed_total_number``, default 4000 synapses for 10k neurons → low
recurrent compute, so the timer reflects per-``Run()`` overhead rather than being
dominated by synaptic delivery). E sources get 80% of the synapse budget, I
sources 20%, split proportional to population size. Inhibition is ``-g`` times
excitation (g=5). One ``poisson_generator`` drives all neurons (identical drive
across every config) so there is real spiking compute. A ``spike_recorder`` on
all neurons provides the equivalence check via its ``n_events`` counter.

METHODOLOGY (matters for honest numbers)
----------------------------------------
* ``local_num_threads = 1`` and a FIXED ``rng_seed`` → deterministic, so the
  bit-identical equivalence check is meaningful.
* The network is rebuilt fresh BEFORE each rep and the build is OUTSIDE the timer;
  only the Run/Simulate phase is timed.
* One untimed warmup per config, then ``--reps`` timed reps. The MIN wall is the
  headline number (least contended); median is also reported.
* Slowdown is ``min_config / min_monolithic``.

REQUIRES NEST
-------------
Needs a working NEST install (``import nest``). Benchmarked on NEST 3.8.0
(OpenMP-only, single MPI rank). CLAUDE.md pins NESTML 8.2.0 → NEST 3.9 as the
target; numbers may shift slightly on 3.9. Run the env interpreter DIRECTLY
(e.g. ``/opt/anaconda3/envs/p2b/bin/python -u bench_chunk_overhead.py``) rather
than ``conda run`` — ``conda run`` fully buffers child stdout when redirected, so
per-row progress never streams.

EXAMPLE
-------
    python bench_chunk_overhead.py --neurons 10000 --synapses 4000 \
        --chunk-ms 100 50 20 10 5 2 1 --t-bio-ms 1000 --reps 5 --strict

    # quick smoke test (tiny net, fast)
    python bench_chunk_overhead.py --neurons 200 --synapses 100 \
        --chunk-ms 100 10 --t-bio-ms 100 --reps 2 --warmup 1 --strict
"""
from __future__ import annotations

import argparse
import json
import statistics
import time


def build_network(args):
    """Build the network fresh. NOT timed. Deterministic via fixed seed.

    Returns the spike_recorder so the caller can read ``n_events`` after a run.
    """
    import nest

    nest.ResetKernel()
    nest.local_num_threads = args.threads
    nest.rng_seed = args.seed

    ne = int(round(args.neurons * 0.8))
    ni = args.neurons - ne
    n_total = ne + ni

    w_exc = args.w_exc
    w_inh = -args.g * args.w_exc

    exc = nest.Create("iaf_psc_alpha", ne)
    inh = nest.Create("iaf_psc_alpha", ni)
    alln = exc + inh

    # Recurrent synapses via fixed_total_number (static_synapse), split
    # proportional to source population size: ~80% E sources, ~20% I sources.
    n_syn_e = int(round(args.synapses * ne / n_total))
    n_syn_i = args.synapses - n_syn_e
    nest.Connect(
        exc, alln,
        conn_spec={"rule": "fixed_total_number", "N": n_syn_e},
        syn_spec={"synapse_model": "static_synapse", "weight": w_exc},
    )
    nest.Connect(
        inh, alln,
        conn_spec={"rule": "fixed_total_number", "N": n_syn_i},
        syn_spec={"synapse_model": "static_synapse", "weight": w_inh},
    )

    # Poisson drive to all neurons -> real spiking compute, identical per config.
    pg = nest.Create("poisson_generator", params={"rate": args.poisson_hz})
    nest.Connect(pg, alln, syn_spec={"weight": w_exc})

    # Spike recorder on all neurons for the equivalence check.
    sr = nest.Create("spike_recorder")
    nest.Connect(alln, sr)

    return sr, n_total, n_syn_e + n_syn_i


def run_monolithic(t_bio_ms):
    """Prepare(); Run(T_bio); Cleanup() — a single Run call."""
    import nest

    nest.Prepare()
    nest.Run(t_bio_ms)
    nest.Cleanup()
    return 1  # n_Run_calls


def run_chunked_efficient(t_bio_ms, chunk_ms):
    """Prepare ONCE, loop Run(chunk); kernel state persists; Cleanup at end.

    This is the NCP control pattern.
    """
    import nest

    n_calls = int(round(t_bio_ms / chunk_ms))
    nest.Prepare()
    for _ in range(n_calls):
        nest.Run(chunk_ms)
    nest.Cleanup()
    return n_calls


def run_chunked_naive(t_bio_ms, chunk_ms):
    """Anti-pattern: each Simulate does Prepare+Run+Cleanup internally."""
    import nest

    n_calls = int(round(t_bio_ms / chunk_ms))
    for _ in range(n_calls):
        nest.Simulate(chunk_ms)
    return n_calls


def time_config(args, runner, *runner_args):
    """``warmup`` untimed reps (discarded) + ``reps`` timed reps.

    The network is rebuilt fresh (UNTIMED) before every rep so each timed run
    starts from an identical, deterministic state.
    """
    for _ in range(args.warmup):
        build_network(args)
        runner(*runner_args)

    times = []
    spikes = []
    n_calls = None
    for _ in range(args.reps):
        sr, _, _ = build_network(args)
        t0 = time.perf_counter()
        n_calls = runner(*runner_args)
        t1 = time.perf_counter()
        times.append(t1 - t0)
        spikes.append(int(sr.get("n_events")))
    return times, spikes, n_calls


def sweep(args) -> dict:
    import nest

    # Probe the actual network shape once (untimed) for the report header.
    _, n_total, n_syn = build_network(args)

    results = []  # list of row dicts

    # MONOLITHIC (the slowdown baseline).
    t, s, nc = time_config(args, run_monolithic, args.t_bio_ms)
    results.append({
        "config": "monolithic", "chunk_ms": args.t_bio_ms, "n_run": nc,
        "min_s": round(min(t), 6), "median_s": round(statistics.median(t), 6),
        "total_spikes": s[0], "spikes_all_reps": s,
    })
    mono_min = min(t)
    mono_spikes = s[0]

    # CHUNKED-EFFICIENT across the chunk sweep.
    for chunk in args.chunk_ms:
        t, s, nc = time_config(args, run_chunked_efficient,
                               args.t_bio_ms, float(chunk))
        results.append({
            "config": "chunked-efficient", "chunk_ms": float(chunk), "n_run": nc,
            "min_s": round(min(t), 6),
            "median_s": round(statistics.median(t), 6),
            "total_spikes": s[0], "spikes_all_reps": s,
        })

    # CHUNKED-NAIVE (anti-pattern) at a single representative chunk size.
    naive_chunk = float(args.naive_chunk_ms)
    t, s, nc = time_config(args, run_chunked_naive, args.t_bio_ms, naive_chunk)
    results.append({
        "config": "chunked-naive", "chunk_ms": naive_chunk, "n_run": nc,
        "min_s": round(min(t), 6), "median_s": round(statistics.median(t), 6),
        "total_spikes": s[0], "spikes_all_reps": s,
    })

    # Fill in slowdown relative to monolithic MIN wall.
    for row in results:
        row["slowdown_x"] = round(row["min_s"] / mono_min, 4) if mono_min else 0.0

    # EQUIVALENCE: monolithic + ALL chunked-efficient reps must be bit-identical.
    eff_spikes = []
    for row in results:
        if row["config"] in ("monolithic", "chunked-efficient"):
            eff_spikes.extend(row["spikes_all_reps"])
    all_identical = all(v == mono_spikes for v in eff_spikes)

    return {
        "nest_version": nest.__version__,
        "network": {
            "neurons": n_total, "synapses": n_syn,
            "poisson_hz": args.poisson_hz, "t_bio_ms": args.t_bio_ms,
            "threads": args.threads, "seed": args.seed,
        },
        "method": {"warmup": args.warmup, "reps": args.reps,
                   "headline": "MIN wall (rebuild fresh untimed before each rep)"},
        "results": results,
        "equivalence": {
            "monolithic_spikes": mono_spikes,
            "all_efficient_identical": all_identical,
            "checked_reps": len(eff_spikes),
        },
    }


def main() -> None:
    p = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--neurons", type=int, default=10000,
                   help="total neurons (80%% E / 20%% I)")
    p.add_argument("--synapses", type=int, default=4000,
                   help="total recurrent synapses (fixed_total_number)")
    p.add_argument("--chunk-ms", type=float, nargs="+",
                   default=[100, 50, 20, 10, 5, 2, 1],
                   help="chunk sizes (ms) to sweep for chunked-efficient")
    p.add_argument("--naive-chunk-ms", type=float, default=10.0,
                   help="chunk size for the chunked-naive anti-pattern row")
    p.add_argument("--t-bio-ms", type=float, default=1000.0,
                   help="biological time integrated per rep")
    p.add_argument("--reps", type=int, default=5, help="timed reps (MIN reported)")
    p.add_argument("--warmup", type=int, default=1,
                   help="untimed warmup reps per config")
    p.add_argument("--threads", type=int, default=1,
                   help="local_num_threads (1 for deterministic equivalence)")
    p.add_argument("--poisson-hz", type=float, default=8000.0,
                   help="poisson_generator rate driving all neurons")
    p.add_argument("--w-exc", type=float, default=20.0, help="excitatory weight pA")
    p.add_argument("--g", type=float, default=5.0,
                   help="inhibitory weight is -g * w_exc")
    p.add_argument("--seed", type=int, default=12345)
    p.add_argument("--strict", action="store_true",
                   help="exit non-zero if the spike-count equivalence check fails")
    p.add_argument("--out", type=str, default=None,
                   help="write JSON results to this file (creates parent dirs)")
    args = p.parse_args()

    try:
        import nest  # noqa: F401
    except ImportError:
        raise SystemExit(
            "This benchmark REQUIRES NEST (import nest failed). "
            "Install NEST (benchmarked on 3.8.0) and run the env interpreter "
            "directly, e.g. /opt/anaconda3/envs/p2b/bin/python -u "
            "bench_chunk_overhead.py")

    report = sweep(args)

    # Human-readable table (streams row by row when run with -u).
    print("NEST_VERSION", report["nest_version"])
    net = report["network"]
    print("NETWORK %d neurons, %d synapses (sparse), poisson %.0f Hz, "
          "T=%.0f ms, threads=%d, seed=%d"
          % (net["neurons"], net["synapses"], net["poisson_hz"],
             net["t_bio_ms"], net["threads"], net["seed"]))
    print("METHOD %d warmup + %d timed reps; rebuild fresh (untimed) per rep; "
          "MIN reported" % (args.warmup, args.reps))
    print("%-20s %10s %8s %12s %12s %10s %12s" %
          ("config", "chunk_ms", "n_Run", "min_s", "median_s",
           "slowdownX", "total_spikes"))
    for row in report["results"]:
        print("ROW %-16s %10.0f %8d %12.6f %12.6f %10.4f %12d" %
              (row["config"], row["chunk_ms"], row["n_run"], row["min_s"],
               row["median_s"], row["slowdown_x"], row["total_spikes"]),
              flush=True)

    eq = report["equivalence"]
    print("EQUIVALENCE monolithic_spikes=%d all_efficient_identical=%s "
          "(checked %d reps)"
          % (eq["monolithic_spikes"], eq["all_efficient_identical"],
             eq["checked_reps"]), flush=True)

    print(json.dumps(report, indent=2))

    if args.out:
        import os
        os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
        with open(args.out, "w") as f:
            json.dump(report, f, indent=2)
        print(f"Wrote results to {args.out}", flush=True)

    if not eq["all_efficient_identical"]:
        print("CORRECTNESS_PROBLEM spike counts diverge across "
              "monolithic/chunked-efficient!", flush=True)
        if args.strict:
            raise SystemExit(1)
    print("DONE", flush=True)


if __name__ == "__main__":
    main()
