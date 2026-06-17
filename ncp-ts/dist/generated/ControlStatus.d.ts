import type { Mode } from "./Mode";
/**
 * Controller → plant / telemetry: loop health and mode.
 */
export type ControlStatus = {
    ncp_version: string;
    kind: string;
    seq: bigint;
    t: number;
    mode: Mode;
    sim_time_ms: number;
    loop_latency_ms: number;
    safety_ok: boolean;
    note: string | null;
};
//# sourceMappingURL=ControlStatus.d.ts.map