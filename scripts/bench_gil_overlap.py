#!/usr/bin/env python3
"""bench_gil_overlap.py — does a NATIVE thread overlap transport with nest.Run()?

WHAT THIS MEASURES
  nest.Run() holds the CPython GIL for essentially its whole duration (the NEST
  kernel is C++, but the PyNEST/Cython wrapper does not release the GIL around it).
  This script measures whether transport-like work can run *concurrently with*
  nest.Run() when it is placed on:
    - serial          : the same Python thread, after Run            (baseline)
    - native thread   : a C pthread (built + loaded here via ctypes) -- a faithful
                        proxy for a Rust std::thread / a PyO3 background thread,
                        all of which are native OS threads that never hold the GIL
    - python thread   : a threading.Thread doing pure-Python busy work

  Expected result: the native thread overlaps Run (speedup > 1), the Python thread
  does not (~1.0, modulo NEST's brief internal GIL releases). This is the evidence
  behind NCP keeping transport in the Rust gateway / a PyO3 background thread rather
  than the NEST Python thread (see PERFORMANCE.md and ROADMAP.md).

METHODOLOGY
  Per variant: a fresh NEST network is built (OUTSIDE the timer), warmed up, then the
  N-chunk loop is timed; MIN over --reps is reported. The "transport work" is a fixed
  CPU busy-spin of --work-ms (a stand-in for serialize + send); only its *location*
  (native vs python vs serial) changes. The C submit() is non-blocking: it signals a
  worker pthread and returns, so the worker spins DURING the following nest.Run().

EQUIVALENCE / FAIRNESS
  All three variants do identical total work (N nest.Run(chunk) + N busy-spins of
  work-ms); only whether the busy-spin overlaps Run differs. ctypes releases the GIL
  around C calls, so submit() returns immediately and the worker is GIL-free.

REQUIRES NEST (and a C compiler: cc/clang/gcc). Run the env interpreter directly
  (NOT `conda run`, which buffers child stdout):
      /path/to/envs/p2b/bin/python -u scripts/bench_gil_overlap.py

EXAMPLE
      python -u scripts/bench_gil_overlap.py --n 8000 --threads 8 \
              --chunk-ms 20 --work-ms 10 --n-chunks 30 --reps 3
"""
from __future__ import annotations

import argparse
import ctypes
import os
import platform
import statistics
import subprocess
import sys
import tempfile
import threading
import time

# Native worker: a pthread that busy-spins for a submitted duration. submit() is
# non-blocking (signals the worker and returns); the spin runs on the worker thread,
# which never touches the GIL -- exactly how a Rust std::thread / PyO3 bg thread runs.
_C_SRC = r"""
#include <pthread.h>
#include <time.h>
static pthread_t TH;
static pthread_mutex_t M = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t  CV = PTHREAD_COND_INITIALIZER;
static long JOB_US = 0; static int HAS_JOB = 0, WORKING = 0, STOP = 0;
static double now_us(void){ struct timespec t; clock_gettime(CLOCK_MONOTONIC,&t);
    return (double)t.tv_sec*1e6 + (double)t.tv_nsec/1e3; }
static void busy(long us){ double s = now_us(); while (now_us() - s < (double)us){} }
static void* loop(void* _a){ (void)_a;
    for(;;){ pthread_mutex_lock(&M);
        while(!HAS_JOB && !STOP) pthread_cond_wait(&CV,&M);
        if(STOP){ pthread_mutex_unlock(&M); return 0; }
        HAS_JOB = 0; WORKING = 1; long u = JOB_US; pthread_mutex_unlock(&M);
        busy(u);
        pthread_mutex_lock(&M); WORKING = 0; pthread_mutex_unlock(&M);
    } }
void olib_start(void){ pthread_create(&TH,0,loop,0); }
void olib_submit(long us){ pthread_mutex_lock(&M); JOB_US=us; HAS_JOB=1;
    pthread_cond_signal(&CV); pthread_mutex_unlock(&M); }
int  olib_busy(void){ int b; pthread_mutex_lock(&M); b = HAS_JOB||WORKING;
    pthread_mutex_unlock(&M); return b; }
void olib_blocking(long us){ busy(us); }
void olib_stop(void){ pthread_mutex_lock(&M); STOP=1; pthread_cond_signal(&CV);
    pthread_mutex_unlock(&M); pthread_join(TH,0); }
"""


def _build_native_lib() -> ctypes.CDLL:
    cc = os.environ.get("CC", "cc")
    d = tempfile.mkdtemp(prefix="ncp_gil_")
    csrc = os.path.join(d, "olib.c")
    ext = "dylib" if platform.system() == "Darwin" else "so"
    lib = os.path.join(d, f"olib.{ext}")
    with open(csrc, "w") as f:
        f.write(_C_SRC)
    flags = ["-O2", "-dynamiclib"] if platform.system() == "Darwin" else ["-O2", "-shared", "-fPIC"]
    subprocess.run([cc, *flags, "-o", lib, csrc], check=True)
    c = ctypes.CDLL(lib)
    c.olib_submit.argtypes = [ctypes.c_long]
    c.olib_blocking.argtypes = [ctypes.c_long]
    c.olib_busy.restype = ctypes.c_int
    return c


def _build_net(nest, n: int, threads: int, poisson_hz: float, seed: int):
    try:
        nest.Cleanup()
    except Exception:
        pass
    nest.ResetKernel()
    nest.local_num_threads = threads
    nest.rng_seed = seed
    ne, ni = int(n * 0.8), int(n * 0.2)
    exc = nest.Create("iaf_psc_alpha", ne)
    inh = nest.Create("iaf_psc_alpha", ni)
    alln = exc + inh
    pg = nest.Create("poisson_generator", params={"rate": poisson_hz})
    nest.Connect(pg, alln, syn_spec={"weight": 20.0})
    nest.Connect(exc, alln, {"rule": "fixed_indegree", "indegree": 400},
                 {"weight": 20.0, "delay": 1.5})
    nest.Connect(inh, alln, {"rule": "fixed_indegree", "indegree": 100},
                 {"weight": -100.0, "delay": 1.5})
    nest.Prepare()


def main() -> int:
    ap = argparse.ArgumentParser(description="Native-vs-Python-thread overlap with nest.Run().")
    ap.add_argument("--n", type=int, default=8000)
    ap.add_argument("--threads", type=int, default=8)
    ap.add_argument("--chunk-ms", type=float, default=20.0)
    ap.add_argument("--work-ms", type=float, default=10.0)
    ap.add_argument("--n-chunks", type=int, default=30)
    ap.add_argument("--reps", type=int, default=3)
    ap.add_argument("--poisson-hz", type=float, default=2800.0)
    ap.add_argument("--seed", type=int, default=12345)
    ap.add_argument("--out", type=str, default=None,
                    help="write JSON results to this file (creates parent dirs)")
    a = ap.parse_args()

    try:
        import nest  # noqa
    except Exception as exc:  # pragma: no cover
        sys.exit(f"REQUIRES NEST: `import nest` failed ({exc}). Install NEST 3.x.")

    c = _build_native_lib()
    c.olib_start()
    chunk, work_us, N = a.chunk_ms, int(a.work_ms * 1000), a.n_chunks

    def warmup():
        for _ in range(3):
            nest.Run(chunk)

    def serial():
        t = time.perf_counter()
        for _ in range(N):
            nest.Run(chunk)
            c.olib_blocking(work_us)
        return time.perf_counter() - t

    def native_overlap():
        t = time.perf_counter()
        for _ in range(N):
            c.olib_submit(work_us)        # non-blocking: worker spins during Run
            nest.Run(chunk)
            while c.olib_busy():
                pass
        return time.perf_counter() - t

    def py_busy(us):
        s = time.perf_counter()
        while (time.perf_counter() - s) * 1e6 < us:
            pass

    def py_thread_overlap():
        t = time.perf_counter()
        for _ in range(N):
            th = threading.Thread(target=py_busy, args=(work_us,))
            th.start()
            nest.Run(chunk)
            th.join()
        return time.perf_counter() - t

    def measure(fn):
        _build_net(nest, a.n, a.threads, a.poisson_hz, a.seed)
        warmup()
        return min(fn() for _ in range(a.reps))

    # per-chunk compute
    _build_net(nest, a.n, a.threads, a.poisson_hz, a.seed)
    warmup()
    compute_ms = statistics.median(
        ((lambda: (lambda t0: (nest.Run(chunk), time.perf_counter() - t0)[1])(time.perf_counter()))()
         for _ in range(10))
    ) * 1000.0

    s = measure(serial)
    no = measure(native_overlap)
    pt = measure(py_thread_overlap)
    c.olib_stop()

    print(f"RESULT compute_ms={compute_ms:.2f} chunk_ms={chunk} work_ms={a.work_ms} "
          f"n_chunks={N} threads={a.threads} n={a.n}")
    print(f"RESULT serial_s={s:.4f} native_overlap_s={no:.4f} py_thread_overlap_s={pt:.4f}")
    print(f"RESULT native_speedup={s/no:.3f} py_thread_speedup={s/pt:.3f}")
    print(f"RESULT expected_serial_s~={N*(compute_ms+a.work_ms)/1000:.4f} "
          f"expected_native_s~={N*max(compute_ms,a.work_ms)/1000:.4f}")
    print("NOTE native thread (C pthread here) == Rust std::thread / PyO3 bg thread: "
          "no GIL, overlaps nest.Run; Python thread is GIL-bound.")

    if a.out:
        import json as _json, os as _os
        _os.makedirs(_os.path.dirname(a.out) or ".", exist_ok=True)
        report = {
            "nest_version": nest.__version__,
            "compute_ms": round(compute_ms, 2),
            "chunk_ms": chunk, "work_ms": a.work_ms,
            "n_chunks": N, "threads": a.threads, "n": a.n,
            "serial_s": round(s, 4),
            "native_overlap_s": round(no, 4),
            "py_thread_overlap_s": round(pt, 4),
            "native_speedup": round(s / no, 3),
            "py_thread_speedup": round(s / pt, 3),
        }
        with open(a.out, "w") as f:
            _json.dump(report, f, indent=2)
        print(f"Wrote results to {a.out}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
