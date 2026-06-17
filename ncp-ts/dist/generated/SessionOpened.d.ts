import type { SimProvenance } from "./SimProvenance";
/**
 * Ack of `open_session` with resolved sizes and provenance.
 */
export type SessionOpened = {
    ncp_version: string;
    kind: string;
    session_id: string;
    ok: boolean;
    backend: string;
    resolved: {
        [key in string]: bigint;
    };
    provenance: SimProvenance | null;
    error: string | null;
};
//# sourceMappingURL=SessionOpened.d.ts.map