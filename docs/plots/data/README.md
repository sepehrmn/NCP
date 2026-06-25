# Benchmark data audit trail

This directory holds machine-generated JSON results from the NCP benchmark
scripts. `scripts/plot_perf.py` reads them (when present) to generate the
performance figures in `docs/plots/`, creating a provenance chain:

```
bench_*.py --out docs/plots/data/<name>.json  →  plot_perf.py  →  docs/plots/*.svg
```

When the JSON files are absent (e.g. NEST is not installed), `plot_perf.py`
falls back to hardcoded constants transcribed from `PERFORMANCE.md` and
`NEST_REALTIME.md`, so SVG generation never breaks.

## Regenerating the audit trail

```bash
# Real-time factor sweep (requires NEST)
python3 scripts/bench_realtime.py --out docs/plots/data/realtime.json \
    --n 10000 50000 100000 200000 --threads 1 2 4 8 16 --reps 3

# GIL overlap (requires NEST + a C compiler for the ctypes native lib)
python3 scripts/bench_gil_overlap.py --out docs/plots/data/gil_overlap.json

# Overlap ceiling (requires NEST)
python3 scripts/bench_overlap.py --out docs/plots/data/overlap.json

# Chunk overhead (requires NEST)
python3 scripts/bench_chunk_overhead.py --out docs/plots/data/chunk_overhead.json

# Regenerate the SVGs (picks up the data files automatically)
python3 scripts/plot_perf.py
```

## File format

Each JSON file is the direct output of the corresponding benchmark script's
`--out` flag — the same JSON that is also printed to stdout. The schema is
script-specific (see each script's docstring), but all include a `nest_version`
field for reproducibility.
