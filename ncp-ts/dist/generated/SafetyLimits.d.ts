/**
 * Bounds the action plane. `max_speed_mps`, `geofence_radius_m` and
 * `command_timeout_ms` are enforced by the action-plane safety governor;
 * `max_tilt_rad` is advisory metadata and is **not** enforced in this layer
 * (no command-path clamp consumes it yet).
 */
export type SafetyLimits = {
    max_speed_mps: number | null;
    max_tilt_rad: number | null;
    geofence_radius_m: number | null;
    command_timeout_ms: number;
};
//# sourceMappingURL=SafetyLimits.d.ts.map