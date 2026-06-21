#!/usr/bin/env python3
"""Cross-language, cross-process NCP end-to-end runner (live transport, NEST-free).

This is the "full picture" integration run: it stands up the **real** Python NCP
server (engram's `bridge_server --backend mock` — `SessionService` + the NEST-free
`MockBackend`, exposed over a real localhost-TCP socket with newline-delimited JSON,
the exact medium the Rust `ncp-gateway` bridges) and drives it from clients in
**different languages**, asserting each completes the open→step→close lifecycle with
the contract intact (handshake, the scientific-boundary discriminators).

It proves what a single repo's CI cannot: that a Rust consumer (a crebain/pid_vla
peer) interoperates with the Python engram server across a genuine process +
language boundary — without needing NEST or zenoh-python. The backend is orthogonal
to the wire: `MockBackend` emits real `Observation` frames, so this is a faithful
test of the *medium + contract*, which is what a release must guarantee regardless
of the simulator behind it.

Cross-PROCESS over the production Zenoh transport is covered separately and gates in
CI: `ncp-zenoh/tests/cross_session_rpc.rs` (two real Zenoh sessions over a tcp link).

Requirements (a local integration runner, not a single-repo CI gate): this NCP repo
+ a sibling `Paper2Brain` checkout (override with `--engram PATH` or `$P2B_ROOT`),
`cargo`, and a Python with `pydantic`. Skips with a clear message if any is absent.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path

NCP_ROOT = Path(__file__).resolve().parents[1]


def _wire_version() -> str:
    """The current wire version, read from the behavior corpus (the single
    cross-language source of wire truth) so this runner tracks the wire across a
    bump instead of hardcoding a literal that goes stale on the next cut."""
    corpus = NCP_ROOT / "conformance" / "behavior" / "vectors.json"
    return json.loads(corpus.read_text())["ncp_version"]


def _free_port() -> int:
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def _wait_listen(port: int, proc: subprocess.Popen, tries: int = 60) -> None:
    for _ in range(tries):
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.3).close()
            return
        except OSError:
            if proc.poll() is not None:
                out = proc.stdout.read() if proc.stdout else ""
                raise RuntimeError(f"server exited early:\n{out}")
            time.sleep(0.1)
    raise RuntimeError("server did not start listening")


def _python_client(port: int, wire: str) -> bool:
    """A Python NCP client: open -> step -> close over the socket, asserting contract."""
    with socket.create_connection(("127.0.0.1", port), timeout=2) as c:
        f = c.makefile("r")

        def rpc(msg: dict) -> dict:
            c.sendall((json.dumps(msg) + "\n").encode())
            return json.loads(f.readline())

        opened = rpc({"kind": "open_session", "ncp_version": wire, "session_id": "py-cli",
                      "network": {"kind": "builtin", "ref": "iaf_psc_alpha"}})
        ok = (opened.get("kind") == "session_opened" and opened.get("ok") is True
              and opened["provenance"]["is_simulation_output"] is True
              and opened["provenance"]["calibrated_posterior"] is False)
        obs = rpc({"kind": "step_request", "ncp_version": wire, "session_id": "py-cli", "advance_ms": 10.0})
        ok = ok and obs.get("kind") == "observation_frame" and obs["is_simulation_output"] is True
        closed = rpc({"kind": "close_session", "ncp_version": wire, "session_id": "py-cli"})
        ok = ok and closed.get("ok") is True
        return ok


def main() -> int:
    ap = argparse.ArgumentParser(description="Cross-language NCP e2e against the Python server.")
    ap.add_argument("--engram", default=os.environ.get("P2B_ROOT", str(NCP_ROOT.parent / "Paper2Brain")),
                    help="path to the Paper2Brain (engram) checkout")
    args = ap.parse_args()
    engram = Path(args.engram)

    if not (engram / "backend" / "neurocontrol" / "bridge_server.py").exists():
        print(f"SKIP: engram checkout not found at {engram} (set --engram / $P2B_ROOT). "
              "Component halves still gate: ncp-zenoh cross_session_rpc + the behavioral corpus.")
        return 0

    # Build the Rust client (proves a Rust peer drives the Python server). Distinguish
    # "no toolchain" (legitimate skip) from "cargo present but build BROKE" (a real
    # failure — e.g. API drift — that must NOT pass green).
    if shutil.which("cargo") is None:
        print("SKIP: cargo not on PATH (no Rust toolchain).")
        return 0
    rust_bin = NCP_ROOT / "target" / "debug" / "examples" / "ncp_tcp_client"
    print("building the Rust NCP client …")
    if subprocess.run(["cargo", "build", "-p", "ncp-core", "--example", "ncp_tcp_client"],
                      cwd=NCP_ROOT).returncode != 0:
        print("FAIL: the Rust e2e client failed to build (cargo present) — API drift?")
        return 1

    port = _free_port()
    srv = subprocess.Popen([sys.executable, "-m", "backend.neurocontrol.bridge_server",
                            "--backend", "mock", "--port", str(port)],
                           cwd=str(engram), stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    results: dict[str, bool] = {}
    wire = _wire_version()
    try:
        _wait_listen(port, srv)
        # Python client
        try:
            results["python"] = _python_client(port, wire)
        except Exception as exc:  # noqa: BLE001
            print(f"python client error: {exc}")
            results["python"] = False
        # Rust client (separate process, separate language, same server + wire)
        r = subprocess.run([str(rust_bin), str(port)], capture_output=True, text=True, timeout=30)
        print(r.stdout.strip() or r.stderr.strip())
        results["rust"] = r.returncode == 0
    finally:
        srv.terminate()
        try:
            srv.wait(timeout=5)
        except subprocess.TimeoutExpired:
            srv.kill()

    print("\n=== cross-language e2e vs the Python engram server (MockBackend) ===")
    for lang, ok in results.items():
        print(f"  {lang:8} {'PASS' if ok else 'FAIL'}")
    all_ok = all(results.values()) and len(results) >= 2
    print("RESULT:", "PASS" if all_ok else "FAIL")
    return 0 if all_ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
