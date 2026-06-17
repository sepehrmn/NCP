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
export const NCP_VERSION = '0.1'

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
    })
    return unwrap<SessionOpenedReply>(reply)
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
