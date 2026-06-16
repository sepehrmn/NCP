#!/usr/bin/env bash
# NCP conformance / smoke check — runs the full SDK test matrix across all
# languages. Run from anywhere: `ncp/scripts/check.sh`. Network is needed the
# first time to fetch/build Zenoh and the binding deps.
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n=== %s ===\n' "$1"; }

step "ncp-core (pure: wire-compat, codec, safety, keys, control loop)"
cargo test -p ncp-core

step "ncp-core → TypeScript bindings (ts-rs; regenerates ncp-core/bindings/*.ts)"
cargo test -p ncp-core --features ts

step "ncp-zenoh build (transport: queryable RPC + 3 pub/sub planes, QoS)"
cargo build -p ncp-zenoh

step "ncp-zenoh loopback (real Zenoh runtime: streaming control loop round trip)"
cargo test -p ncp-zenoh --test loopback

step "ncp-gateway build (Engram's Rust edge → Python bridge)"
cargo build -p ncp-gateway

step "ncp-python build (PyO3 binding)"
cargo build -p ncp-python

step "ncp-cpp build (C ABI for C++; cbindgen header)"
cargo build -p ncp-cpp

step "clippy (lints)"
cargo clippy -p ncp-core --all-targets

printf '\n✅ ALL NCP CHECKS PASSED\n'
