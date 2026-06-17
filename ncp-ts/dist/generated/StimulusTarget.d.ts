import type { StimulusKind } from "./StimulusKind";
/**
 * One stimulus input port.
 */
export type StimulusTarget = {
    port: string;
    target: string;
    kind: StimulusKind;
    ids: Array<bigint>;
    /**
     * Named stimulus parameters beyond the scalar value, e.g. siegert_neuron's
     * diffusion_connection `drift_factor` / `diffusion_factor`. (#10)
     */
    params: {
        [key in string]: number;
    };
};
//# sourceMappingURL=StimulusTarget.d.ts.map