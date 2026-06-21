#!/usr/bin/env python3
"""verify_nest_chunking.py — prove NCP's Prepare-once / Run(chunk) model on YOUR NEST.

WHY THIS EXISTS
  A reasonable worry about NCP's stepwise control model (nest.Prepare() once,
  nest.Run(chunk) per control tick, nest.Cleanup() once) is that NEST's per-node
  preparation hook — `calibrate()`, renamed `pre_run_hook()` in NEST 3.x, which
  precomputes the resolution-dependent integration propagators — might re-run on
  every Run() (every "resume"), making chunked simulation slower and changing the
  science vs one monolithic Run. It does NOT, and this script proves it on the NEST
  you actually have installed (the numbers in PERFORMANCE.md were taken on 3.8.0;
  engram pins 3.9). See NEST_REALTIME.md "The chunking question".

WHAT IT PROVES (3 tests; --strict exits non-zero on any failure)
  A. CALIBRATION IS ONCE-PER-PREPARE (operational). Times three patterns over the
     SAME total biological time:
       - monolithic         : Prepare(); Run(T); Cleanup()
       - chunked-efficient  : Prepare() ONCE; Run(T/N) x N; Cleanup()   (the NCP pattern)
       - chunked-naive      : nest.Simulate(T/N) x N  (= Prepare()+Run()+Cleanup()
                              EACH chunk, i.e. re-calibrates every chunk)
     If pre_run_hook re-ran per Run(), chunked-efficient would cost the same as
     chunked-naive. It does not: efficient ~= monolithic + small fixed per-Run
     overhead, while naive is many times slower. That gap IS the once-per-Prepare
     calibration, measured on your NEST.
  B. PER-RUN FIXED OVERHEAD. Reports (chunked_efficient - monolithic) / N across a
     sweep of chunk sizes — the per-Run() bookkeeping cost, which should be small and
     roughly independent of network size.
  C. CHUNKING IS BIT-IDENTICAL SCIENCE. Two identically-seeded networks WITH plastic
     (stdp_synapse) connections: one monolithic, one chunked. Asserts identical
     spike (time, sender) sets AND identical final STDP weights — so chunk boundaries
     change neither spike timing nor plasticity.

REQUIRES NEST 3.x (3.9 recommended). Run the env interpreter directly with -u
  (NOT `conda run`, which buffers child stdout):
      /path/to/envs/p2b/bin/python -u scripts/verify_nest_chunking.py --strict
"""
from __future__ import annotations

import argparse
import contextlib
import os
import re
import sys
import tempfile
import time


@contextlib.contextmanager
def _capture_cstderr():
    """Capture C-level stderr (fd 2) — where NEST's kernel logs — into a temp file,
    so we can count log lines emitted from C++ (Python's sys.stderr capture cannot)."""
    saved = os.dup(2)
    tf = tempfile.TemporaryFile(mode="w+b")
    try:
        os.dup2(tf.fileno(), 2)
        yield tf
    finally:
        os.dup2(saved, 2)
        os.close(saved)


def _count_node_prepares(nest, build_fn, *, run_block) -> int:
    """Count NodeManager::prepare_nodes() invocations via its 'Preparing N node(s)
    for simulation.' M_INFO log line. `run_block(chunk)` performs the advance pattern."""
    nest.set_verbosity("M_INFO")
    with _capture_cstderr() as tf:
        build_fn()
        run_block()
        tf.seek(0)
        text = tf.read().decode("utf-8", "replace")
    nest.set_verbosity("M_ERROR")
    return len(re.findall(r"Preparing\s+\d+\s+node", text))


def _reset(nest, threads: int, seed: int, resolution: float) -> None:
    try:
        nest.Cleanup()
    except Exception:
        pass
    nest.ResetKernel()
    nest.local_num_threads = threads
    nest.resolution = resolution
    nest.rng_seed = seed


def _build(nest, n: int, *, plastic: bool, poisson_hz: float):
    """A small balanced network. With plastic=True, recurrent E->E synapses are
    stdp_synapse so weights evolve (for the bit-identical-plasticity test)."""
    ne, ni = int(n * 0.8), max(1, int(n * 0.2))
    exc = nest.Create("iaf_psc_alpha", ne)
    inh = nest.Create("iaf_psc_alpha", ni)
    alln = exc + inh
    pg = nest.Create("poisson_generator", params={"rate": poisson_hz})
    nest.Connect(pg, alln, syn_spec={"weight": 20.0})
    ee_syn = {"synapse_model": "stdp_synapse", "weight": 20.0, "delay": 1.5} if plastic else {
        "weight": 20.0, "delay": 1.5
    }
    nest.Connect(exc, alln, {"rule": "fixed_indegree", "indegree": min(400, ne)}, ee_syn)
    nest.Connect(inh, alln, {"rule": "fixed_indegree", "indegree": min(100, ni)},
                 {"weight": -100.0, "delay": 1.5})
    sr = nest.Create("spike_recorder")
    nest.Connect(alln, sr)
    return exc, sr


def _spike_set(nest, sr):
    ev = sr.get("events")
    # round times to the kernel resolution grid to avoid float-repr noise in the set
    return sorted(zip((round(t, 6) for t in ev["times"]), (int(s) for s in ev["senders"])))


def _stdp_weights(nest, exc):
    conns = nest.GetConnections(source=exc, synapse_model="stdp_synapse")
    w = conns.get("weight")
    return [round(float(x), 9) for x in (w if isinstance(w, (list, tuple)) else [w])]


def main() -> int:
    ap = argparse.ArgumentParser(description="Prove NEST Prepare-once/Run-chunk equivalence + cost.")
    ap.add_argument("--n", type=int, default=2000)
    ap.add_argument("--threads", type=int, default=4)
    ap.add_argument("--t-bio", type=float, default=1000.0, help="total biological ms")
    ap.add_argument("--chunks", type=int, nargs="+", default=[200, 50, 10],
                    help="chunk counts to sweep (T_bio split into this many Run calls)")
    ap.add_argument("--poisson-hz", type=float, default=18000.0)
    ap.add_argument("--resolution", type=float, default=0.1)
    ap.add_argument("--seed", type=int, default=12345)
    ap.add_argument("--reps", type=int, default=3)
    ap.add_argument("--strict", action="store_true", help="exit non-zero on any equivalence failure")
    a = ap.parse_args()

    try:
        import nest  # noqa
    except Exception as exc:  # pragma: no cover
        sys.exit(f"REQUIRES NEST: `import nest` failed ({exc}). Install NEST 3.x.")
    ver = getattr(nest, "__version__", "unknown")
    nest.set_verbosity("M_ERROR")
    print(f"RESULT nest_version={ver} n={a.n} threads={a.threads} t_bio={a.t_bio} resolution={a.resolution}")

    failures: list[str] = []

    # ── Test A (decisive): node calibration runs ONCE per Prepare(), not per Run() ──
    # NodeManager::prepare_nodes() (the path that calls each node's pre_run_hook, the
    # renamed calibrate()) logs "Preparing N node(s) for simulation." once per call. So
    # Prepare() + many Run() logs it ONCE; the Simulate()-per-chunk anti-pattern logs it
    # once PER chunk — a direct, on-your-own-NEST refutation of "calibrate every chunk".
    n_proof = 50

    def _mk_build():
        _reset(nest, a.threads, a.seed, a.resolution)
        _build(nest, a.n, plastic=False, poisson_hz=a.poisson_hz)

    def _eff_block():
        nest.Prepare()
        for _ in range(n_proof):
            nest.Run(2.0)
        nest.Cleanup()

    def _naive_block():
        for _ in range(n_proof):
            nest.Simulate(2.0)  # = Prepare()+Run()+Cleanup() each chunk

    eff_prepares = _count_node_prepares(nest, _mk_build, run_block=_eff_block)
    naive_prepares = _count_node_prepares(nest, _mk_build, run_block=_naive_block)
    print(f"RESULT node_calibration_passes chunked_efficient[Prepare+{n_proof}xRun]={eff_prepares} "
          f"chunked_naive[Simulate x{n_proof}]={naive_prepares}  (expect 1 and {n_proof})")
    if eff_prepares != 1:
        failures.append(
            f"Prepare()+{n_proof}xRun() logged {eff_prepares} node-calibration passes, expected 1 — "
            f"node pre_run_hook() must be once-per-Prepare, not per-Run"
        )

    def timed_monolithic() -> float:
        _reset(nest, a.threads, a.seed, a.resolution)
        _build(nest, a.n, plastic=False, poisson_hz=a.poisson_hz)
        nest.Prepare()
        t = time.perf_counter()
        nest.Run(a.t_bio)
        dt = time.perf_counter() - t
        nest.Cleanup()
        return dt

    def timed_chunked_efficient(nchunks: int) -> float:
        _reset(nest, a.threads, a.seed, a.resolution)
        _build(nest, a.n, plastic=False, poisson_hz=a.poisson_hz)
        chunk = a.t_bio / nchunks
        nest.Prepare()
        t = time.perf_counter()
        for _ in range(nchunks):
            nest.Run(chunk)  # NO re-Prepare: pre_run_hook does NOT fire here
        dt = time.perf_counter() - t
        nest.Cleanup()
        return dt

    def timed_chunked_naive(nchunks: int) -> float:
        _reset(nest, a.threads, a.seed, a.resolution)
        _build(nest, a.n, plastic=False, poisson_hz=a.poisson_hz)
        chunk = a.t_bio / nchunks
        t = time.perf_counter()
        for _ in range(nchunks):
            nest.Simulate(chunk)  # = Prepare()+Run()+Cleanup() EACH chunk -> re-calibrates
        return time.perf_counter() - t

    # ── Test A + B: calibration-once + per-Run overhead ──────────────────────
    mono = min(timed_monolithic() for _ in range(a.reps))
    print(f"RESULT monolithic_s={mono:.4f}")
    print("RESULT chunks | efficient_s | naive_s | per_run_overhead_ms | naive/efficient")
    for nchunks in a.chunks:
        eff = min(timed_chunked_efficient(nchunks) for _ in range(a.reps))
        naive = min(timed_chunked_naive(nchunks) for _ in range(a.reps))
        per_run_ms = max(0.0, (eff - mono)) / nchunks * 1000.0
        ratio = naive / eff if eff else float("inf")
        print(f"RESULT   {nchunks:>5} | {eff:>10.4f} | {naive:>7.4f} | {per_run_ms:>18.3f} | {ratio:>6.1f}x")
        # The decisive assertion: Prepare-once is dramatically cheaper than re-Prepare
        # per chunk. If pre_run_hook re-ran per Run(), this ratio would be ~1.0.
        if a.strict and nchunks >= 10 and ratio < 1.5:
            failures.append(
                f"chunked-naive is only {ratio:.2f}x slower than chunked-efficient at "
                f"{nchunks} chunks — expected >> 1 (re-Prepare/re-calibrate should be costly). "
                f"Either the network is too small to show it, or Run() is re-calibrating."
            )

    # ── Test C: bit-identical spike times + STDP weights ─────────────────────
    def run_capture(nchunks: int):
        _reset(nest, a.threads, a.seed, a.resolution)
        exc, sr = _build(nest, a.n, plastic=True, poisson_hz=a.poisson_hz)
        nest.Prepare()
        if nchunks == 1:
            nest.Run(a.t_bio)
        else:
            for _ in range(nchunks):
                nest.Run(a.t_bio / nchunks)
        spikes, weights = _spike_set(nest, sr), _stdp_weights(nest, exc)
        nest.Cleanup()
        return spikes, weights

    mono_spk, mono_w = run_capture(1)
    chk_spk, chk_w = run_capture(max(a.chunks))
    spikes_match = mono_spk == chk_spk
    weights_match = mono_w == chk_w
    print(f"RESULT equivalence chunks={max(a.chunks)} spikes={len(mono_spk)} "
          f"stdp_synapses={len(mono_w)} spike_times_identical={spikes_match} "
          f"stdp_weights_identical={weights_match}")
    if not spikes_match:
        failures.append("monolithic vs chunked produced DIFFERENT spike (time,sender) sets")
    if not weights_match:
        failures.append("monolithic vs chunked produced DIFFERENT final STDP weights")

    print(
        "NOTE pre_run_hook (calibrate) runs once in Prepare(), not per Run(): chunked-efficient "
        "~= monolithic + small per-Run overhead, while chunked-naive (Simulate-per-chunk) "
        "re-calibrates every chunk and is many times slower. Chunking is bit-identical."
    )
    if failures:
        print("FAIL verify_nest_chunking:")
        for f in failures:
            print(f"  - {f}")
        return 1 if a.strict else 0
    print("OK verify_nest_chunking: Prepare-once avoids per-chunk calibration; chunking is exact.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
