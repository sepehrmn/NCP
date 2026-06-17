import type { ChannelValue } from "./ChannelValue";
/**
 * Plant → controller: the latest sensed state. Carries `seq`/`t` so a command
 * can be stamped with the sensor it was computed from (the correspondence the
 * split perception/action planes must preserve — join on `seq`, not arrival).
 */
export type SensorFrame = {
    ncp_version: string;
    kind: string;
    seq: bigint;
    t: number;
    frame_id: string;
    channels: {
        [key in string]: ChannelValue;
    };
};
//# sourceMappingURL=SensorFrame.d.ts.map