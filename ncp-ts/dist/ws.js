/**
 * WebSocket transport for the NCP client. The session service replies to each
 * message in order, so requests are correlated FIFO. Use this `send` with
 * `NeuroSimClient`, or implement `Send` over another bus (e.g. Zenoh) instead.
 */
export class WebSocketNeuroSim {
    ws;
    pending = [];
    ready;
    closedError = null;
    constructor(url = 'ws://127.0.0.1:28471/api/neurocontrol/ws') {
        this.ws = new WebSocket(url);
        let rejectReady;
        this.ready = new Promise((resolve, reject) => {
            rejectReady = reject;
            this.ws.onopen = () => resolve();
        });
        this.ws.onmessage = (event) => {
            const pending = this.pending.shift();
            if (!pending)
                return;
            try {
                // Parse inside the handler so one malformed frame rejects exactly the
                // request it was dequeued for, keeping FIFO correlation in sync.
                pending.resolve(JSON.parse(event.data));
            }
            catch (error) {
                pending.reject(new Error(`NCP reply was not valid JSON: ${WebSocketNeuroSim.messageOf(error)}`));
            }
        };
        // A close or error after connection must settle every in-flight request,
        // otherwise awaiting NeuroSimClient calls would hang forever. The same
        // handler rejects the `ready` promise if the socket never opened.
        this.ws.onerror = () => {
            const error = new Error('NCP WebSocket error');
            rejectReady(error); // no-op once `ready` has resolved
            this.failAll(error);
        };
        this.ws.onclose = () => {
            this.failAll(new Error('NCP WebSocket closed'));
        };
    }
    static messageOf(error) {
        return error instanceof Error ? error.message : String(error);
    }
    /** Reject and drop every queued request; new sends fail fast afterwards. */
    failAll(error) {
        if (!this.closedError) {
            this.closedError = error;
        }
        while (this.pending.length > 0) {
            this.pending.shift().reject(this.closedError);
        }
    }
    /** Transport-agnostic `send` for `NeuroSimClient`. */
    send = async (message) => {
        await this.ready;
        if (this.closedError) {
            throw this.closedError;
        }
        return new Promise((resolve, reject) => {
            const request = { resolve, reject };
            this.pending.push(request);
            try {
                this.ws.send(JSON.stringify(message));
            }
            catch (error) {
                const index = this.pending.indexOf(request);
                if (index !== -1)
                    this.pending.splice(index, 1);
                reject(new Error(`NCP send failed: ${WebSocketNeuroSim.messageOf(error)}`));
            }
        });
    };
    close() {
        this.failAll(new Error('NCP WebSocket closed by client'));
        this.ws.close();
    }
}
//# sourceMappingURL=ws.js.map