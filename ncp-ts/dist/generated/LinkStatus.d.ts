/**
 * Link-health telemetry from the seq-gap / CUSUM monitor (published on the
 * control plane). `burst=true` flags sustained loss — a possible jam — at which
 * point the only sound response is to fail safe, not add redundancy.
 */
export type LinkStatus = {
    ncp_version: string;
    kind: string;
    session_id: string;
    t: number;
    last_seq: bigint;
    received: bigint;
    lost: bigint;
    loss_rate: number;
    burst: boolean;
};
//# sourceMappingURL=LinkStatus.d.ts.map