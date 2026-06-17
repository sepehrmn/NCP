/**
 * WebSocket transport for the NCP client. The session service replies to each
 * message in order, so requests are correlated FIFO. Use this `send` with
 * `NeuroSimClient`, or implement `Send` over another bus (e.g. Zenoh) instead.
 */
import type { Send } from './client';
export declare class WebSocketNeuroSim {
    private readonly ws;
    private readonly pending;
    private readonly ready;
    private closedError;
    constructor(url?: string);
    private static messageOf;
    /** Reject and drop every queued request; new sends fail fast afterwards. */
    private failAll;
    /** Transport-agnostic `send` for `NeuroSimClient`. */
    readonly send: Send;
    close(): void;
}
//# sourceMappingURL=ws.d.ts.map