// Copy the ts-rs-generated bindings (ncp-core/bindings/*.ts, generated from the
// ncp-core reference types, which conform to the normative proto/ncp.proto wire
// contract) into this package's src/generated/ so the package is self-contained
// for git / npm consumption.
//
// Run after regenerating the bindings:
//   cargo test -p ncp-core --features ts   # rewrites ncp-core/bindings/*.ts
//   node ncp-ts/scripts/sync-bindings.mjs  # mirrors them here
//   tsc -p ncp-ts/tsconfig.json            # rebuilds ncp-ts/dist
// (or just `npm run regen` from the repo root, which chains all three).
import { readdirSync, copyFileSync, rmSync, mkdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const here = dirname(fileURLToPath(import.meta.url)) // ncp-ts/scripts
const src = join(here, '..', '..', 'ncp-core', 'bindings') // ncp-core/bindings
const dst = join(here, '..', 'src', 'generated') // ncp-ts/src/generated

rmSync(dst, { recursive: true, force: true })
mkdirSync(dst, { recursive: true })
const files = readdirSync(src).filter((f) => f.endsWith('.ts'))
for (const f of files) copyFileSync(join(src, f), join(dst, f))
console.log(`synced ${files.length} bindings: ncp-core/bindings -> ncp-ts/src/generated`)
