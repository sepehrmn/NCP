#!/usr/bin/env python3
"""Drive 5 distinct NEST spiking networks through the NCP RPC contract.

Speaks newline-delimited JSON to engram's bridge_server (--backend nest) — the
exact medium the NCP Rust gateway bridges from Zenoh. open_session ->
step_request* (current_pA stimulus, spikes recording) -> close_session.
"""
import json, socket, sys, time

HOST, PORT = "127.0.0.1", 28474
NCP = "0.5"
HASH = "24e8e6e31e1dec8a"

NETWORKS = [
    # (label, nest model, pop size, [current_pA per 100ms step])
    ("iaf_psc_alpha (current LIF)",   "iaf_psc_alpha",   10, [500.0, 750.0, 1000.0]),
    ("iaf_psc_exp (exp-synapse LIF)", "iaf_psc_exp",     10, [500.0, 750.0, 1000.0]),
    ("izhikevich (regular spiking)",  "izhikevich",       8, [10.0, 15.0, 20.0]),
    ("hh_psc_alpha (Hodgkin-Huxley)", "hh_psc_alpha",     6, [650.0, 800.0, 1000.0]),
    ("aeif_cond_alpha (adaptive EIF)","aeif_cond_alpha",  6, [500.0, 750.0, 1000.0]),
]

def rpc(sock, rdr, msg):
    sock.sendall((json.dumps(msg) + "\n").encode())
    line = rdr.readline()
    if not line:
        raise RuntimeError("connection closed by bridge")
    return json.loads(line)

def run_one(sock, rdr, label, model, n, currents):
    sid = f"nest-{model}"
    opened = rpc(sock, rdr, {
        "ncp_version": NCP, "kind": "open_session", "session_id": sid,
        "network": {"kind": "builtin", "ref": model, "population_sizes": {"pop": n}},
        "record": {"targets": [{"port": "spk", "target": "pop", "observable": "spikes"}]},
        "stimulus": {"targets": [{"port": "drive", "target": "pop", "kind": "current_pA"}]},
        "sim": {"dt_ms": 0.1, "chunk_ms": 10.0, "mode": "stream"},
        "bindings": [], "contract_hash": HASH,
    })
    if opened.get("kind") == "error" or opened.get("ok") is False:
        return {"label": label, "model": model, "ok": False, "detail": opened}
    backend = opened.get("backend")
    total_spikes, per_step = 0, []
    for i, cur in enumerate(currents):
        obs = rpc(sock, rdr, {
            "ncp_version": NCP, "kind": "step_request", "session_id": sid, "advance_ms": 100.0,
            "stimulus": {"kind": "stimulus_frame", "session_id": sid, "t": float(i),
                         "values": {"drive": {"data": [cur], "unit": "pA"}}},
        })
        if obs.get("kind") == "error":
            return {"label": label, "model": model, "ok": False, "detail": obs}
        rec = (obs.get("records") or {}).get("spk", {})
        nspk = len(rec.get("times", []) or [])
        total_spikes += nspk
        per_step.append((cur, nspk, obs.get("sim_time_ms")))
    closed = rpc(sock, rdr, {"ncp_version": NCP, "kind": "close_session", "session_id": sid})
    return {"label": label, "model": model, "ok": True, "backend": backend,
            "pop": n, "total_spikes": total_spikes, "per_step": per_step,
            "closed_ok": closed.get("ok")}

def main():
    t0 = time.time()
    with socket.create_connection((HOST, PORT), timeout=120) as s:
        s.settimeout(120)
        rdr = s.makefile("r")
        results = []
        for (label, model, n, currents) in NETWORKS:
            try:
                results.append(run_one(s, rdr, label, model, n, currents))
            except Exception as e:
                results.append({"label": label, "model": model, "ok": False, "detail": str(e)})
    print(f"\n=== 5 NEST spiking sims via NCP (NEST backend), {time.time()-t0:.1f}s ===")
    print(f"{'network':32} {'pop':>4} {'spikes':>7}  steps(curr_pA->spikes)")
    print("-" * 86)
    ok = 0
    for r in results:
        if r.get("ok"):
            ok += 1
            steps = " ".join(f"{int(c)}->{ns}" for (c, ns, _) in r["per_step"])
            print(f"{r['label']:32} {r['pop']:>4} {r['total_spikes']:>7}  {steps}")
        else:
            print(f"{r['label']:32}  FAILED: {str(r.get('detail'))[:80]}")
    print("-" * 86)
    print(f"backend={results[0].get('backend') if results else '?'}  ok={ok}/{len(results)}")
    sys.exit(0 if ok == len(results) else 1)

if __name__ == "__main__":
    main()
