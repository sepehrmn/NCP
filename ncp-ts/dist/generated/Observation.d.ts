import type { Observable } from "./Observable";
/**
 * Recorded data for one record port. `times`+`values` are parallel for analog;
 * `times`+`senders` are parallel for spikes.
 */
export type Observation = {
    port: string;
    target: string;
    observable: Observable;
    times: Array<number>;
    values: Array<number>;
    senders: Array<bigint>;
    unit: string | null;
    /**
     * Which named recordable this series carries (e.g. `g_ex`, `w`) when a port
     * records more than the primary `observable`; `None` = the `observable`. (#10)
     */
    recordable: string | null;
};
//# sourceMappingURL=Observation.d.ts.map