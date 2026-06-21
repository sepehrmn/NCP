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

**Additive vs incompatible (since v0.4 — see [VERSIONING.md](VERSIONING.md)).** Adding
an *optional* field or a new message type is **non-breaking** — unknown fields are
ignored, so it does **not** require an `ncp_version` bump (it does change
`CONTRACT_HASH`, an advisory signal). An **incompatible** change — removing/renaming a
field, changing a type, removing an enum value, or changing the meaning of existing
bytes — is breaking and **must** include, in the same PR:

1. An explicit **`ncp_version` bump**. The version constant lives in
   `ncp-core/src/messages.rs` (`NCP_VERSION`, currently `"0.4"`).
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

## Releases and tags (no moved tags)

Release tags are **immutable**. Once a `vX.Y.Z` tag is pushed, never delete,
re-point, or force-update it — downstream consumers (each self-registered via a
`.ncp-consumer` descriptor; see [`INTEGRATING.md`](INTEGRATING.md)) and the
`@sepehrmn/ncp` package pin NCP by tag, and a moved tag silently changes the bytes
those pins resolve to. If a release is wrong, cut a **new** tag (bump the version);
do not rewrite history under an existing one.

A release tag must be coherent: the Cargo workspace version, `package.json`
version, and `CITATION.cff` version must all equal the tag's version, and the
annotated tag must peel to the exact commit it was cut from. The read-only
`scripts/check-version-coherence.sh` guard asserts this (run it with the tag
name, e.g. `scripts/check-version-coherence.sh v0.2.8`); the **version
coherence** CI job runs it on every PR and on tag pushes. Before bumping
downstream pins, run `scripts/check-consumer-pins.sh` from a full local tree to
confirm every consumer agrees on the target tag.

### Release runbook (run BEFORE you cut the tag)

A tag is immutable, so everything must be green *before* it exists — never tag, then
fix, then move. The exact pre-tag sequence:

1. **Bump every version site together** to the target `X.Y.Z`: `Cargo.toml`
   (`[workspace.package]` + the path-dep `version=`), `package.json`,
   `ncp-ts/package.json`, `CITATION.cff`, and the `README.md` pins/bibtex.
2. **Bump the wire constants together** *only if the wire changed*: `NCP_VERSION`
   in **both** `ncp-core/src/messages.rs` and `ncp-ts/src/client.ts` (the
   coherence guard checks they agree — this is the skew that shipped once);
   recompute/update `CONTRACT_HASH` if the proto's wire-semantic content changed.
3. **Regenerate everything**: `bun run regen` (ts bindings + dist) and
   `cargo run -p ncp-core --features schema --bin gen-schemas` (schemas).
4. **Run the full gate** — `scripts/check.sh` — plus `scripts/check-version-coherence.sh`
   and (from a full local tree) `scripts/check-consumer-pins.sh`. All must be green.
5. **Only now** cut the annotated tag at that commit and push it. If anything is
   wrong, fix it and bump to the next patch — **do not move the tag**.

Patch releases (docs/tests/additive, wire unchanged) do **not** require a consumer
re-pin: by the versioning policy, same-`MAJOR.MINOR` peers interoperate, and
`check-consumer-pins.sh` treats a patch difference as compatible.

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
