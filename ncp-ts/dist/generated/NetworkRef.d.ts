import type { NetworkRefKind } from "./NetworkRefKind";
/**
 * What to simulate.
 */
export type NetworkRef = {
    kind: NetworkRefKind;
    /**
     * builtin model name, or a `compiled_module_id` (kind=handle). `ref` is a
     * Rust keyword, so the field is `ref_` and renamed on the wire.
     */
    ref: string;
    /**
     * kind=handle: which registered model to create if the handle has >1.
     */
    model_name: string | null;
    population_sizes: {
        [key in string]: bigint;
    };
    params: {
        [key in string]: number;
    };
};
//# sourceMappingURL=NetworkRef.d.ts.map