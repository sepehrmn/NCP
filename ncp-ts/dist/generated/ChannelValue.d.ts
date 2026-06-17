/**
 * A channel sample: a flat list of floats plus an optional unit string. Width
 * carries the semantics (1=scalar, 3=vec3, 4=quat, N=array).
 */
export type ChannelValue = {
    data: Array<number>;
    unit: string | null;
};
//# sourceMappingURL=ChannelValue.d.ts.map