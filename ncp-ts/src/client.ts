/**
 * Neuro-Cybernetic Protocol (NCP) — transport-agnostic TypeScript client.
 *
 * Wire-identical to the normative `proto/ncp.proto` contract (proto-native) and the
 * Rust (`ncp-core`) and Python peers: every reply and enum type is imported from
 * the generated bindings (`./generated`, the ts-rs output of the `ncp-core`
 * reference types). This file adds only the *client* orchestration (build a
 * request, await the typed reply) and a JSON-wire view of the generated types.
 * Request envelopes are built as object literals — keep their fields in sync with
 * the generated request types (`OpenSession`/`StepRequest`/`RunRequest`/`CloseSession`).
 *
 * Transport-agnostic: provide any `send(message) => Promise<reply>` (see `ws.ts`
 * for a WebSocket implementation; a Zenoh/native transport can implement the same
 * `Send`).
 */

import type {
  ChannelValue,
  NetworkRef,
  Observation,
  ObservationFrame,
  RecordTarget,
  SessionClosed,
  SessionOpened,
  SimConfig,
  StimulusTarget,
} from './generated'

/** The protocol version this client stamps on every request (`ncp_version`). */
export const NCP_VERSION = '0.4'

/**
 * This peer's contract-hash (`ncp_core::CONTRACT_HASH` — FNV-1a of the canonicalized
 * proto). Pinned, cross-language-anchored to the Rust/Python peers and verified
 * against the proto in those peers' CI. Carried in `open()` and compared to the
 * server's reply as an **advisory** signal (see `contractStatus`): a mismatch is
 * surfaced, not thrown — `ncp_version` is the hard compatibility gate.
 */
export const NCP_CONTRACT_HASH = '2cf0763ad61e4f1c'

/** Advisory comparison of a peer-advertised contract hash to ours. Mirrors
 *  `ncp_core::contract_status` — never throws; `null` = match or not advertised, a
 *  string = an advisory message describing the mismatch (for logging/telemetry). */
export function contractStatus(peerHash: string | null | undefined): string | null {
  if (peerHash == null || peerHash === NCP_CONTRACT_HASH) return null
  return (
    `NCP contract-hash differs: peer ${JSON.stringify(peerHash)}, ours ` +
    `${JSON.stringify(NCP_CONTRACT_HASH)} — versions compatible so the session ` +
    `proceeds, but the peers are on different contract revisions (advisory)`
  )
}

/** Thrown when a peer's `ncp_version` is unparseable or incompatible (the HARD
 *  compatibility gate — distinct from the advisory contract-hash check). */
export class NcpVersionError extends Error {}

/** Parse a wire version into `[major, minor]`, mirroring `ncp_core::check_version`'s
 *  parser: 1 or 2 dot-separated base-10 components, no trailing junk, no third
 *  component (semver patch is not part of the wire id). A missing minor ("1") is
 *  minor 0; anything else throws — never silently coerced to 0 (that would turn the
 *  fail-closed guard fail-open the moment our own minor is 0). */
function parseMajorMinor(version: string): [number, number] {
  const fail = (): never => {
    throw new NcpVersionError(`unparseable ncp_version ${JSON.stringify(version)}`)
  }
  const parts = version.split('.')
  if (parts.length < 1 || parts.length > 2) fail()
  const part = (s: string | undefined): number => {
    // Base-10 non-negative integer only — reject empty/undefined, signs, whitespace,
    // junk (matches Rust's `u64::parse`, which `check_version` rejects, not coerces).
    if (s === undefined || !/^[0-9]+$/.test(s) || !Number.isSafeInteger(Number(s))) return fail()
    return Number(s)
  }
  return [part(parts[0]), parts.length === 2 ? part(parts[1]) : 0]
}

/**
 * The HARD wire-compatibility gate — `true` if `version` can speak our wire.
 * Mirrors `ncp_core::check_version` exactly so the TS peer accepts/rejects the same
 * versions as the Rust/Python/C++ peers: for a pre-1.0 wire (major 0) the protocol
 * has no stability guarantee, so BOTH major and minor must match (`0.3 ≠ 0.4`); for
 * a stable wire (major ≥ 1) the major alone decides. An unparseable version always
 * throws [`NcpVersionError`]; an incompatible-but-parseable version throws when
 * `strict`, else returns `false`. This is the gate `contractStatus` is explicitly
 * NOT — the contract hash is advisory; the version is fail-closed.
 */
export function checkVersion(version: string, strict = false): boolean {
  const [gotMajor, gotMinor] = parseMajorMinor(version)
  const [wantMajor, wantMinor] = parseMajorMinor(NCP_VERSION)
  const compatible =
    wantMajor === 0 ? gotMajor === wantMajor && gotMinor === wantMinor : gotMajor === wantMajor
  if (!compatible) {
    if (strict) {
      throw new NcpVersionError(`NCP version mismatch: got ${version}, want ${NCP_VERSION}`)
    }
    return false
  }
  return true
}

/** Thrown when a frame violates the NCP scientific-boundary discriminators. */
export class NcpScientificBoundaryError extends Error {}

/**
 * Enforce the **mandatory, fail-closed scientific-boundary discriminators** on an
 * inbound `observation_frame` (or a `session_opened.provenance` block): NCP output is
 * a *control artifact*, never a validated reproduction, so `is_simulation_output` MUST
 * be `true` and `calibrated_posterior` MUST be `false`. A TS consumer should call this
 * on frames it reads so a peer cannot quietly hand it a frame claiming calibrated /
 * non-simulation status. Mirrors the boundary pins `ncp_core::validate` enforces in the
 * Rust/Python/C++ peers. Throws [`NcpScientificBoundaryError`] on a violation.
 */
export function assertScientificBoundary(frame: Record<string, unknown>): void {
  const kind = frame.kind
  // The discriminators live top-level on observation_frame, and inside
  // session_opened.provenance.
  const carrier =
    kind === 'session_opened' && frame.provenance && typeof frame.provenance === 'object'
      ? (frame.provenance as Record<string, unknown>)
      : frame
  if (!('is_simulation_output' in carrier) && !('calibrated_posterior' in carrier)) {
    return // not a boundary-carrying frame (e.g. a control reply)
  }
  if (carrier.is_simulation_output !== true) {
    throw new NcpScientificBoundaryError(
      `NCP boundary: is_simulation_output must be true (got ${JSON.stringify(carrier.is_simulation_output)}) — output is a control artifact, not a validated reproduction`,
    )
  }
  if (carrier.calibrated_posterior !== false) {
    throw new NcpScientificBoundaryError(
      `NCP boundary: calibrated_posterior must be false (got ${JSON.stringify(carrier.calibrated_posterior)})`,
    )
  }
}

/**
 * JSON-wire view of a canonical type. ts-rs emits Rust `i64` fields (ids,
 * `population_sizes`, `senders`, `resolved`, `seq`, `seed`, …) as `bigint` for
 * precision-safety, but `JSON.stringify` cannot serialize a `bigint` and
 * `JSON.parse` yields `number`; NCP uses small integers, so the JSON wire uses
 * `number` (see `ncp-core/bindings/README.md`). `Wire<T>` maps `bigint → number`
 * recursively so the generated shapes stay wire-identical to the contract while
 * remaining JSON-(de)serializable.
 */
export type Wire<T> = T extends bigint
  ? number
  : T extends Array<infer U>
    ? Array<Wire<U>>
    : T extends object
      ? { [K in keyof T]: Wire<T[K]> }
      : T

// JSON-wire aliases of the reply types consumers read back off the wire.
export type SessionOpenedReply = Wire<SessionOpened>
export type SessionClosedReply = Wire<SessionClosed>
export type ObservationFrameReply = Wire<ObservationFrame>
export type ObservationData = Wire<Observation>

/**
 * Construction views. The canonical message types are maximally strict (ts-rs
 * marks every Rust field required), but the JSON Schemas default most fields, so
 * for *building* a request we make the defaulted members optional while keeping
 * the discriminating members required. The client fills the envelope
 * (`kind`, `ncp_version`, `session_id`, empty `bindings`).
 */
export type ChannelInput = Pick<Wire<ChannelValue>, 'data'> & Partial<Wire<ChannelValue>>
export type NetworkInput = Pick<Wire<NetworkRef>, 'kind' | 'ref'> & Partial<Wire<NetworkRef>>
export type RecordInput = Pick<Wire<RecordTarget>, 'port' | 'target' | 'observable'> &
  Partial<Wire<RecordTarget>>
export type StimulusInput = Pick<Wire<StimulusTarget>, 'port' | 'target' | 'kind'> &
  Partial<Wire<StimulusTarget>>
export type SimInput = Partial<Wire<SimConfig>>

/** Any transport: serialize `message`, deliver it to the NCP session service, and
 *  resolve with the reply payload (already parsed from the wire). */
export type Send = (message: Record<string, unknown>) => Promise<unknown>

/**
 * The session service replies to a failed request with one `{ kind: 'error', … }`
 * frame (and keeps the socket open). `unwrap` surfaces it as a thrown error instead
 * of letting an error-shaped object masquerade as a success reply.
 */
export interface ErrorFrame {
  kind: 'error'
  error: string
  session_id?: string | null
}

function unwrap<T>(reply: unknown): T {
  if (reply && typeof reply === 'object' && (reply as { kind?: unknown }).kind === 'error') {
    throw new Error(`NCP error: ${(reply as ErrorFrame).error}`)
  }
  return reply as T
}

export class NeuroSimClient {
  constructor(private readonly send: Send) {}

  /** Open a session: declare what to record and what to stimulate. */
  async open(
    sessionId: string,
    network: NetworkInput,
    record: RecordInput[],
    stimulus: StimulusInput[],
    sim: SimInput = {},
  ): Promise<SessionOpenedReply> {
    // The JSON wire carries int64 as a JSON number, so a seed above 2^53 would
    // lose precision before it reaches NEST. Fail fast rather than silently
    // diverge the RNG. (See proto/ncp.proto header.)
    if (sim.seed != null && !Number.isSafeInteger(sim.seed)) {
      throw new Error(`NCP: sim.seed must be a safe integer (<= 2^53-1); got ${sim.seed}`)
    }
    const reply = await this.send({
      kind: 'open_session',
      ncp_version: NCP_VERSION,
      session_id: sessionId,
      network,
      record: { targets: record },
      stimulus: { targets: stimulus },
      sim,
      bindings: [],
      contract_hash: NCP_CONTRACT_HASH,
    })
    const opened = unwrap<SessionOpenedReply>(reply)
    // Advisory contract-hash check (the reply half): log a mismatch, do not throw —
    // the version is the hard gate (mirrors the NCP session-service contract).
    const advisory = contractStatus((opened as { contract_hash?: string | null }).contract_hash)
    if (advisory) console.warn(`[ncp] ${advisory}`)
    return opened
  }

  /** Advance one chunk; optionally inject `stimulus`; returns an observation frame. */
  async step(
    sessionId: string,
    stimulus: Record<string, ChannelInput> = {},
    advanceMs?: number,
  ): Promise<ObservationFrameReply> {
    const reply = await this.send({
      kind: 'step_request',
      ncp_version: NCP_VERSION,
      session_id: sessionId,
      advance_ms: advanceMs ?? null,
      stimulus: {
        kind: 'stimulus_frame',
        ncp_version: NCP_VERSION,
        session_id: sessionId,
        values: stimulus,
      },
    })
    return unwrap<ObservationFrameReply>(reply)
  }

  /** Batch: advance `durationMs` holding `stimulus`; returns an observation frame. */
  async run(
    sessionId: string,
    durationMs: number,
    stimulus: Record<string, ChannelInput> = {},
  ): Promise<ObservationFrameReply> {
    const reply = await this.send({
      kind: 'run_request',
      ncp_version: NCP_VERSION,
      session_id: sessionId,
      duration_ms: durationMs,
      stimulus: {
        kind: 'stimulus_frame',
        ncp_version: NCP_VERSION,
        session_id: sessionId,
        values: stimulus,
      },
    })
    return unwrap<ObservationFrameReply>(reply)
  }

  /** Close the session. */
  async close(sessionId: string): Promise<SessionClosedReply> {
    const reply = await this.send({
      kind: 'close_session',
      ncp_version: NCP_VERSION,
      session_id: sessionId,
    })
    return unwrap<SessionClosedReply>(reply)
  }
}
