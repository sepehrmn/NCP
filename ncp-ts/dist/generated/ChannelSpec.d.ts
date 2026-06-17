import type { ChannelKind } from "./ChannelKind";
/**
 * Declares a named channel a controller produces or consumes.
 */
export type ChannelSpec = {
    name: string;
    kind: ChannelKind;
    unit: string | null;
    size: bigint | null;
    optional: boolean;
    description: string | null;
};
//# sourceMappingURL=ChannelSpec.d.ts.map