import type { ChannelValue } from "./ChannelValue";
/**
 * The values to inject this step (keyed by stimulus port).
 */
export type StimulusFrame = {
    ncp_version: string;
    kind: string;
    session_id: string;
    t: number;
    values: {
        [key in string]: ChannelValue;
    };
};
//# sourceMappingURL=StimulusFrame.d.ts.map