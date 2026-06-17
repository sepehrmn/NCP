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
/** The protocol version this client stamps on every request (`ncp_version`). */
export const NCP_VERSION = '0.2';
function unwrap(reply) {
    if (reply && typeof reply === 'object' && reply.kind === 'error') {
        throw new Error(`NCP error: ${reply.error}`);
    }
    return reply;
}
export class NeuroSimClient {
    send;
    constructor(send) {
        this.send = send;
    }
    /** Open a session: declare what to record and what to stimulate. */
    async open(sessionId, network, record, stimulus, sim = {}) {
        // The JSON wire carries int64 as a JSON number, so a seed above 2^53 would
        // lose precision before it reaches NEST. Fail fast rather than silently
        // diverge the RNG. (See proto/ncp.proto header.)
        if (sim.seed != null && !Number.isSafeInteger(sim.seed)) {
            throw new Error(`NCP: sim.seed must be a safe integer (<= 2^53-1); got ${sim.seed}`);
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
        });
        return unwrap(reply);
    }
    /** Advance one chunk; optionally inject `stimulus`; returns an observation frame. */
    async step(sessionId, stimulus = {}, advanceMs) {
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
        });
        return unwrap(reply);
    }
    /** Batch: advance `durationMs` holding `stimulus`; returns an observation frame. */
    async run(sessionId, durationMs, stimulus = {}) {
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
        });
        return unwrap(reply);
    }
    /** Close the session. */
    async close(sessionId) {
        const reply = await this.send({
            kind: 'close_session',
            ncp_version: NCP_VERSION,
            session_id: sessionId,
        });
        return unwrap(reply);
    }
}
//# sourceMappingURL=client.js.map