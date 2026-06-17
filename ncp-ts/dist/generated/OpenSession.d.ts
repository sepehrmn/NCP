import type { EntityBinding } from "./EntityBinding";
import type { NetworkRef } from "./NetworkRef";
import type { RecordSpec } from "./RecordSpec";
import type { SimConfig } from "./SimConfig";
import type { StimulusSpec } from "./StimulusSpec";
/**
 * Request a simulation: declare what to record and what to stimulate.
 */
export type OpenSession = {
    ncp_version: string;
    kind: string;
    session_id: string;
    network: NetworkRef;
    record: RecordSpec;
    stimulus: StimulusSpec;
    sim: SimConfig;
    bindings: Array<EntityBinding>;
};
//# sourceMappingURL=OpenSession.d.ts.map