/**
 * Scientific-boundary discriminators carried on every opened session. Returned
 * data is a **raw simulation output of a specified model**, never a validated
 * reproduction: `calibrated_posterior=false`, `is_simulation_output=true`.
 */
export type SimProvenance = {
    network_ref: string;
    backend: string;
    seed: bigint | null;
    calibrated_posterior: boolean;
    is_simulation_output: boolean;
    advisory_only: boolean;
    note: string | null;
};
//# sourceMappingURL=SimProvenance.d.ts.map