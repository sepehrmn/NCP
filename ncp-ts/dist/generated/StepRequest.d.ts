import type { StimulusFrame } from "./StimulusFrame";
/**
 * Advance one chunk; optional stimulus; returns an `ObservationFrame`.
 */
export type StepRequest = {
    ncp_version: string;
    kind: string;
    session_id: string;
    advance_ms: number | null;
    stimulus: StimulusFrame | null;
};
//# sourceMappingURL=StepRequest.d.ts.map