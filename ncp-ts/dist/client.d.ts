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
import type { ChannelValue, NetworkRef, Observation, ObservationFrame, RecordTarget, SessionClosed, SessionOpened, SimConfig, StimulusTarget } from './generated';
/** The protocol version this client stamps on every request (`ncp_version`). */
export declare const NCP_VERSION = "0.4";
/**
 * This peer's contract-hash (`ncp_core::CONTRACT_HASH` — FNV-1a of the canonicalized
 * proto). Pinned, cross-language-anchored to the Rust/Python peers and verified
 * against the proto in those peers' CI. Carried in `open()` and compared to the
 * server's reply as an **advisory** signal (see `contractStatus`): a mismatch is
 * surfaced, not thrown — `ncp_version` is the hard compatibility gate.
 */
export declare const NCP_CONTRACT_HASH = "2cf0763ad61e4f1c";
/** Advisory comparison of a peer-advertised contract hash to ours. Mirrors
 *  `ncp_core::contract_status` — never throws; `null` = match or not advertised, a
 *  string = an advisory message describing the mismatch (for logging/telemetry). */
export declare function contractStatus(peerHash: string | null | undefined): string | null;
/** Thrown when a peer's `ncp_version` is unparseable or incompatible (the HARD
 *  compatibility gate — distinct from the advisory contract-hash check). */
export declare class NcpVersionError extends Error {
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
export declare function checkVersion(version: string, strict?: boolean): boolean;
/** Thrown when a frame violates the NCP scientific-boundary discriminators. */
export declare class NcpScientificBoundaryError extends Error {
}
/**
 * Enforce the **mandatory, fail-closed scientific-boundary discriminators** on an
 * inbound `observation_frame` (or a `session_opened.provenance` block): NCP output is
 * a *control artifact*, never a validated reproduction, so `is_simulation_output` MUST
 * be `true` and `calibrated_posterior` MUST be `false`. A TS consumer should call this
 * on frames it reads so a peer cannot quietly hand it a frame claiming calibrated /
 * non-simulation status. Mirrors the boundary pins `ncp_core::validate` enforces in the
 * Rust/Python/C++ peers. Throws [`NcpScientificBoundaryError`] on a violation.
 */
export declare function assertScientificBoundary(frame: Record<string, unknown>): void;
/**
 * JSON-wire view of a canonical type. ts-rs emits Rust `i64` fields (ids,
 * `population_sizes`, `senders`, `resolved`, `seq`, `seed`, …) as `bigint` for
 * precision-safety, but `JSON.stringify` cannot serialize a `bigint` and
 * `JSON.parse` yields `number`; NCP uses small integers, so the JSON wire uses
 * `number` (see `ncp-core/bindings/README.md`). `Wire<T>` maps `bigint → number`
 * recursively so the generated shapes stay wire-identical to the contract while
 * remaining JSON-(de)serializable.
 */
export type Wire<T> = T extends bigint ? number : T extends Array<infer U> ? Array<Wire<U>> : T extends object ? {
    [K in keyof T]: Wire<T[K]>;
} : T;
export type SessionOpenedReply = Wire<SessionOpened>;
export type SessionClosedReply = Wire<SessionClosed>;
export type ObservationFrameReply = Wire<ObservationFrame>;
export type ObservationData = Wire<Observation>;
/**
 * Construction views. The canonical message types are maximally strict (ts-rs
 * marks every Rust field required), but the JSON Schemas default most fields, so
 * for *building* a request we make the defaulted members optional while keeping
 * the discriminating members required. The client fills the envelope
 * (`kind`, `ncp_version`, `session_id`, empty `bindings`).
 */
export type ChannelInput = Pick<Wire<ChannelValue>, 'data'> & Partial<Wire<ChannelValue>>;
export type NetworkInput = Pick<Wire<NetworkRef>, 'kind' | 'ref'> & Partial<Wire<NetworkRef>>;
export type RecordInput = Pick<Wire<RecordTarget>, 'port' | 'target' | 'observable'> & Partial<Wire<RecordTarget>>;
export type StimulusInput = Pick<Wire<StimulusTarget>, 'port' | 'target' | 'kind'> & Partial<Wire<StimulusTarget>>;
export type SimInput = Partial<Wire<SimConfig>>;
/** Any transport: serialize `message`, deliver it to the NCP session service, and
 *  resolve with the reply payload (already parsed from the wire). */
export type Send = (message: Record<string, unknown>) => Promise<unknown>;
/**
 * The session service replies to a failed request with one `{ kind: 'error', … }`
 * frame (and keeps the socket open). `unwrap` surfaces it as a thrown error instead
 * of letting an error-shaped object masquerade as a success reply.
 */
export interface ErrorFrame {
    kind: 'error';
    error: string;
    session_id?: string | null;
}
export declare class NeuroSimClient {
    private readonly send;
    constructor(send: Send);
    /** Open a session: declare what to record and what to stimulate. */
    open(sessionId: string, network: NetworkInput, record: RecordInput[], stimulus: StimulusInput[], sim?: SimInput): Promise<SessionOpenedReply>;
    /** Advance one chunk; optionally inject `stimulus`; returns an observation frame. */
    step(sessionId: string, stimulus?: Record<string, ChannelInput>, advanceMs?: number): Promise<ObservationFrameReply>;
    /** Batch: advance `durationMs` holding `stimulus`; returns an observation frame. */
    run(sessionId: string, durationMs: number, stimulus?: Record<string, ChannelInput>): Promise<ObservationFrameReply>;
    /** Close the session. */
    close(sessionId: string): Promise<SessionClosedReply>;
}
//# sourceMappingURL=client.d.ts.map