// Behavioral conformance — the TypeScript peer vs the shared decision corpus
// (conformance/behavior/vectors.json), the same corpus the Rust reference is pinned
// to (ncp-core/tests/behavior_conformance.rs) and the Python/C++ peers replay.
//
// ncp-ts is the THIN client: it implements the handshake/boundary PRINCIPLES
// (checkVersion = the hard version gate, contractStatus = the advisory contract
// check, assertScientificBoundary = the boundary discriminators) but not the
// action-plane safety governor or the required-field validator — those live in
// ncp-core / ncp-python / ncp-cpp. This runner replays the corpus functions ncp-ts
// DOES implement and proves it decides identically.
//
// Fail-loud coverage: every corpus function must be either implemented here or in an
// explicit out-of-scope allowlist; a corpus function in NEITHER set is a hard error,
// so a future corpus addition can never silently shrink TS coverage. Reads the single
// repo-root corpus (NOT a vendored copy) and asserts its header pins match ncp-ts's.
//
// Run after `npm run build` (imports the published dist surface):
//   node ncp-ts/scripts/check-behavior.mjs

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
// Import the concrete dist/client.js (not the index barrel): tsconfig uses
// `moduleResolution: Bundler`, so the emitted barrel re-exports are extensionless
// and node's native ESM loader cannot resolve them. client.js is self-contained
// (its ./generated imports are type-only and erased at compile), so it loads under
// plain node — exactly the surface these helpers live on.
import {
  NCP_VERSION,
  NCP_CONTRACT_HASH,
  checkVersion,
  NcpVersionError,
  contractStatus,
  assertScientificBoundary,
  NcpScientificBoundaryError,
} from '../dist/client.js'

const here = dirname(fileURLToPath(import.meta.url)) // ncp-ts/scripts
const corpusPath = join(here, '..', '..', 'conformance', 'behavior', 'vectors.json')
const corpus = JSON.parse(readFileSync(corpusPath, 'utf8'))

const failures = []
const check = (cond, msg) => {
  if (!cond) failures.push(msg)
}

// The TS peer's pinned constants must match the corpus header.
check(
  corpus.ncp_version === NCP_VERSION,
  `corpus ncp_version ${corpus.ncp_version} != ncp-ts ${NCP_VERSION}`,
)
check(
  corpus.contract_hash === NCP_CONTRACT_HASH,
  `corpus contract_hash ${corpus.contract_hash} != ncp-ts ${NCP_CONTRACT_HASH}`,
)

// Functions ncp-ts implements vs deliberately out-of-scope for the thin client.
// `validate` is implemented for the scientific-boundary subset only (the
// required-field half is owned by the full peers).
const IMPLEMENTED = new Set(['check_version', 'contract_status', 'validate'])
const OUT_OF_SCOPE = new Set(['govern']) // action-plane safety governor
for (const fn of Object.keys(corpus.cases)) {
  check(
    IMPLEMENTED.has(fn) || OUT_OF_SCOPE.has(fn),
    `corpus function ${fn} is neither implemented in ncp-ts nor in the out-of-scope ` +
      `allowlist — update ncp-ts/scripts/check-behavior.mjs (do not let it silently drop)`,
  )
}

let covered = 0

// check_version — full parity with the reference gate.
for (const c of corpus.cases.check_version) {
  const { name, input, expect } = c
  let got
  let threw = false
  try {
    got = checkVersion(input.version, input.strict)
  } catch (e) {
    threw = true
    check(e instanceof NcpVersionError, `check_version[${name}]: threw non-NcpVersionError ${e}`)
  }
  if (expect.error) check(threw, `check_version[${name}]: expected throw, got ${got}`)
  else
    check(
      !threw && got === expect.compatible,
      `check_version[${name}]: want compatible=${expect.compatible}, got ${threw ? '<threw>' : got}`,
    )
  covered++
}

// contract_status — ncp-ts collapses match + not_advertised -> null (advisory-clear)
// and mismatch -> an advisory string. Assert the advisory DECISION (warn iff mismatch),
// which is exactly what the client acts on.
for (const c of corpus.cases.contract_status) {
  const { name, input, expect } = c
  const advisory = contractStatus(input.peer_hash)
  check(
    (advisory !== null) === (expect.status === 'mismatch'),
    `contract_status[${name}]: advisory=${JSON.stringify(advisory)} vs expected ${expect.status}`,
  )
  covered++
}

// validate — the thin client enforces the scientific-BOUNDARY discriminators via
// assertScientificBoundary. Cover the boundary-bearing vectors (a violation must
// throw); the required-field-only vectors are out of the thin client's scope.
const hasBoundary = (m) => {
  const carrier = m.kind === 'session_opened' && m.provenance ? m.provenance : m
  return 'is_simulation_output' in carrier || 'calibrated_posterior' in carrier
}
let boundaryCovered = 0
for (const c of corpus.cases.validate) {
  const { name, input, expect } = c
  if (!hasBoundary(input.message)) continue // required-field-only — out of scope
  let threw = false
  try {
    assertScientificBoundary(input.message)
  } catch (e) {
    threw = true
    check(e instanceof NcpScientificBoundaryError, `validate[${name}]: threw non-boundary ${e}`)
  }
  check(threw === !expect.valid, `validate[${name}]: boundary throw=${threw} vs expected valid=${expect.valid}`)
  boundaryCovered++
  covered++
}

const outOfScope =
  corpus.cases.govern.length + (corpus.cases.validate.length - boundaryCovered)
if (failures.length) {
  console.error(`FAIL ncp-ts behavioral conformance: ${failures.length} vector(s) diverged:`)
  for (const f of failures) console.error(`  - ${f}`)
  process.exit(1)
}
console.log(
  `OK ncp-ts behavioral conformance: ${covered} vectors match (check_version + ` +
    `contract_status + scientific-boundary). ${outOfScope} out-of-scope for the thin ` +
    `client (govern + required-field validate) — gated by ncp-core/ncp-python/ncp-cpp.`,
)
