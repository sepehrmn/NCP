import type { StimulusFrame } from "./StimulusFrame";
/**
 * Batch: advance `duration_ms` holding a stimulus; returns an `ObservationFrame`.
 */
export type RunRequest = {
    ncp_version: string;
    kind: string;
    session_id: string;
    duration_ms: number;
    stimulus: StimulusFrame | null;
};
//# sourceMappingURL=RunRequest.d.ts.map