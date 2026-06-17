import type { SimMode } from "./SimMode";
/**
 * Integration / streaming configuration.
 */
export type SimConfig = {
    dt_ms: number;
    chunk_ms: number;
    seed: bigint | null;
    mode: SimMode;
    duration_ms: number | null;
};
//# sourceMappingURL=SimConfig.d.ts.map