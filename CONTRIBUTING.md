# Contributing to NCP

Thanks for your interest in the **Neuro-Cybernetic Protocol (NCP)**. This is
pre-1.0 research software: the wire contract may still change, and the
action/command plane is currently unauthenticated (see [SECURITY.md](SECURITY.md)).
Contributions are welcome, but please read this guide first — NCP ships a
versioned on-the-wire contract, and a few rules below are non-negotiable because
breaking the wire silently breaks every downstream binding and peer.

Be kind. All participation is governed by our
[Code of Conduct](CODE_OF_CONDUCT.md).

## Repository layout

NCP is proto-native: `proto/ncp.proto` is the normative wire contract — the
single source of truth for message structure and the binary encoding. The JSON
Schemas, the Rust/Python/TS/C++ bindings, and `ncp-core`'s serde types generate
from or are conformance-checked against it (via buf; parity guarded in CI).
`ncp-core` is the reference implementation — it owns BEHAVIOR (codec, safety
governor, keys, version).

| Crate / dir   | What it is                                                            |
| ------------- | -------------------------------------------------------------------- |
| `ncp-core`    | Pure, transport-agnostic protocol: wire types, codec, safety governor, control loop, keys. The reference implementation (behavior). |
| `ncp-zenoh`   | Zenoh transport: queryable RPC + the three QoS-differentiated pub/sub planes. |
| `ncp-gateway` | Rust edge → Python bridge.                                           |
| `ncp-python`  | PyO3 Python bindings.                                                |
| `ncp-cpp`     | C ABI + cbindgen header for C/C++ consumers.                         |
| `proto/`      | `ncp.proto` — the normative wire contract (proto-native source of truth).    |
| `schemas/`    | JSON Schemas — the JSON projection of `proto/ncp.proto` (parity-guarded).     |
| `NEURO_CYBERNETIC_PROTOCOL.md` | The protocol specification.                        |

## Building and testing

You need a recent stable Rust toolchain (with `rustfmt` and `clippy` components).
The first build fetches and compiles Zenoh and binding dependencies, so it needs
network access.

### Rust

```bash
# Build / test the whole workspace except the Python binding
cargo build --workspace --exclude ncp-python
cargo test  --workspace --exclude ncp-python

# Regenerate + test the TypeScript bindings (ts-rs)
cargo test -p ncp-core --features ts
```

`ncp-python` is excluded from the default workspace build/test because it links
against a Python interpreter; build it separately (below).

### Python binding (`ncp-python`)

Use a conda environment and [`maturin`](https://github.com/PyO3/maturin):

```bash
conda create -n ncp python=3.11
conda activate ncp
pip install maturin
maturin develop -m ncp-python/Cargo.toml
```

### One-shot conformance / smoke check

`scripts/check.sh` runs the full SDK matrix across all languages (this is what CI
runs):

```bash
ncp/scripts/check.sh
```

## Mandatory gates

Every change must pass all of these locally before you open a PR; CI enforces
them too. A PR that does not pass these will not be merged.

```bash
cargo fmt --all -- --check        # formatting must be clean
cargo clippy --workspace --all-targets -- -D warnings   # zero warnings
cargo test -p ncp-core            # the wire conformance test MUST pass
```

Two **conformance guards** keep the wire in lock-step: `ncp-core/tests/conformance.rs`
(Rust serde ↔ JSON Schema) and `scripts/check_proto_schema_parity.py`
(`proto/ncp.proto` ↔ JSON Schema — field-set + enum wire-string parity). If either
fails, the wire has drifted — fix the drift, do not weaken the test.

## The wire rule (NON-NEGOTIABLE)

NCP's value is a stable, versioned wire contract that independent peers and
language bindings can rely on. **Never silently break the wire.**

Any change to a wire-visible type, enum variant, field, or key — anything that
alters bytes/JSON on the wire or their meaning — **must** include, in the same PR:

1. An explicit **`ncp_version` bump**. The version constant lives in
   `ncp-core/src/messages.rs` (`NCP_VERSION`, currently `"0.2"`).
2. A corresponding update to the **specification**
   (`NEURO_CYBERNETIC_PROTOCOL.md`), the **`.proto` definitions** (`proto/`),
   and the **JSON Schemas** (`schemas/`).
3. A corresponding update to the **conformance test**
   (`ncp-core/tests/conformance.rs`) so it pins the new contract.
4. A **rebuild of the prebuilt TS package** (`bun run regen`, or `bun run build`
   for a source-only change) and a commit of the regenerated `ncp-ts/dist` — it is
   git-tracked and shipped as `@sepehrmn/ncp`, so a stale `dist` would announce the
   wrong wire to JS/TS peers. The `ts dist up-to-date` CI job enforces this.

If your change touches the wire and you have not done all four, it is incomplete.
When in doubt about whether something is wire-visible, assume it is and ask in
the PR.

## Project-agnostic rule

The SDK must stay **project- and vendor-neutral**. Do not introduce
project-specific, application-specific, or vendor-specific code, names,
identifiers, hostnames, or assumptions into any NCP crate. NCP is a general
protocol SDK; downstream applications build on it, not the other way around.

## Benchmarks (require NEST)

The benchmark scripts under `scripts/` measure end-to-end behavior against a real
simulator and therefore **require a working [NEST](https://www.nest-simulator.org/)
installation**:

```bash
python scripts/bench_realtime.py
python scripts/bench_overlap.py
```

They are not part of the mandatory CI gates — run them when your change could
affect latency, throughput, or the real-time control loop, and report the
before/after numbers in your PR.

## Commits and pull requests

- Use [Conventional Commits](https://www.conventionalcommits.org/) for commit
  messages (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `perf:`,
  `build:`, …). Wire-breaking changes should be called out clearly (e.g. a
  `feat!:` / `BREAKING CHANGE:` footer alongside the `ncp_version` bump).
- Keep PRs **small and focused** — one logical change per PR. Large, mixed PRs
  are hard to review and slow to merge.
- Update docs, `CHANGELOG.md`, and tests when your change affects behavior or any
  public/wire contract.
- Make sure the mandatory gates pass before requesting review.
- Do not add Claude, AI assistants, or agents as commit/PR co-authors — no
  `Co-Authored-By:` trailer and no "Generated with Claude Code" / 🤖 line in
  commit messages or pull-request descriptions.

## Developer Certificate of Origin (sign-off)

By contributing you certify that you wrote the patch or otherwise have the right
to submit it under the project's license (the
[Developer Certificate of Origin](https://developercertificate.org/)). Add a
`Signed-off-by` line to each commit:

```bash
git commit -s -m "fix: ..."
```

This appends `Signed-off-by: Your Name <your@email>` using your `git`
`user.name`/`user.email`. PRs whose commits are not signed off may be asked to
amend before merge.
