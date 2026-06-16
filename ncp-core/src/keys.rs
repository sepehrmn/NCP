//! The NCP key scheme — the three-plane addressing the transport bindings use.
//!
//! Perception and action are **separate planes** with different QoS, rate and
//! safety requirements, so they ride separate keys. The control-plane RPC
//! (session lifecycle) is a fourth, rare, request/reply key. Per-entity sub-keys
//! (`…/sensor/imu`, `…/command/cmd_vel`) extend each plane for the multi-sensor /
//! multi-actuator case; subscribers wildcard with `**`.
//!
//! ```text
//! {realm}/rpc                                  control-plane RPC   (queryable)
//! {realm}/session/{id}/sensor[/{name}]         perception plane    (pub/sub, DROP/conflate)
//! {realm}/session/{id}/command[/{name}]         action plane        (pub/sub, reliable+TTL, safety-gated)
//! {realm}/session/{id}/observation              neural/diagnostic   (pub/sub, free observer tap)
//! ```

/// Default realm (key-expression prefix). N Engram instances share one keyspace.
pub const DEFAULT_REALM: &str = "engram/ncp";

/// Is `s` safe to interpolate into a single key segment? A valid segment is
/// non-empty and contains none of the Zenoh key-expression delimiters/wildcards
/// (`/` `*` `$` `#` `?`) nor any ASCII whitespace. The transport boundary
/// (`ncp-zenoh`) rejects on this; the key builders `debug_assert!` on it so a
/// caller passing a wildcard-bearing id (key-injection / cross-session leak) is
/// caught in debug builds.
pub fn valid_id_segment(s: &str) -> bool {
    !s.is_empty()
        && !s
            .chars()
            .any(|c| matches!(c, '/' | '*' | '$' | '#' | '?') || c.is_ascii_whitespace())
}

/// Key-expression builders for a given realm.
#[derive(Clone, Debug)]
pub struct Keys {
    pub realm: String,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            realm: DEFAULT_REALM.to_string(),
        }
    }
}

impl Keys {
    pub fn new(realm: impl Into<String>) -> Self {
        Self {
            realm: realm.into(),
        }
    }

    /// Control-plane RPC queryable (Open/Step/Run/Close).
    pub fn rpc(&self) -> String {
        format!("{}/rpc", self.realm)
    }

    fn session(&self, id: &str) -> String {
        debug_assert!(
            valid_id_segment(id),
            "session id {id:?} is not a valid key segment"
        );
        format!("{}/session/{}", self.realm, id)
    }

    /// Perception plane for a session (all sensors): `…/session/{id}/sensor`.
    pub fn sensor(&self, id: &str) -> String {
        format!("{}/sensor", self.session(id))
    }

    /// One named sensor on the perception plane: `…/session/{id}/sensor/{name}`.
    pub fn sensor_named(&self, id: &str, name: &str) -> String {
        debug_assert!(
            valid_id_segment(name),
            "sensor name {name:?} is not a valid key segment"
        );
        format!("{}/sensor/{}", self.session(id), name)
    }

    /// Action plane for a session (all actuators): `…/session/{id}/command`.
    pub fn command(&self, id: &str) -> String {
        format!("{}/command", self.session(id))
    }

    /// One named actuator on the action plane: `…/session/{id}/command/{name}`.
    pub fn command_named(&self, id: &str, name: &str) -> String {
        debug_assert!(
            valid_id_segment(name),
            "command name {name:?} is not a valid key segment"
        );
        format!("{}/command/{}", self.session(id), name)
    }

    /// Observation/diagnostic plane (the free read-only observer tap).
    pub fn observation(&self, id: &str) -> String {
        format!("{}/observation", self.session(id))
    }

    /// All of a session's sensors (wildcard) — e.g. Engram subscribing to every
    /// sensor of one UAV, whatever the count: `…/session/{id}/sensor/**`.
    pub fn sensor_glob(&self, id: &str) -> String {
        format!("{}/sensor/**", self.session(id))
    }

    /// All of a session's actuators: `…/session/{id}/command/**`.
    pub fn command_glob(&self, id: &str) -> String {
        format!("{}/command/**", self.session(id))
    }

    /// A wildcard over every plane of a session, e.g. for an observer tap.
    pub fn session_glob(&self, id: &str) -> String {
        format!("{}/**", self.session(id))
    }

    /// Every session in the realm — the fleet wildcard (all UAVs):
    /// `{realm}/session/**`. Per-entity `seq` is scoped to each named
    /// sensor/actuator stream, so a `LinkMonitor`/`ActionBuffer` is instantiated
    /// per `(session, entity)`.
    pub fn fleet_glob(&self) -> String {
        format!("{}/session/**", self.realm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_scheme() {
        let k = Keys::default();
        assert_eq!(k.rpc(), "engram/ncp/rpc");
        assert_eq!(k.sensor("s1"), "engram/ncp/session/s1/sensor");
        assert_eq!(
            k.sensor_named("s1", "imu"),
            "engram/ncp/session/s1/sensor/imu"
        );
        assert_eq!(
            k.command_named("s1", "cmd_vel"),
            "engram/ncp/session/s1/command/cmd_vel"
        );
        assert_eq!(k.observation("s1"), "engram/ncp/session/s1/observation");
        assert_eq!(k.session_glob("s1"), "engram/ncp/session/s1/**");
    }

    #[test]
    fn valid_id_segment_accepts_normal_ids() {
        assert!(valid_id_segment("s1"));
        assert!(valid_id_segment("uav3"));
        assert!(valid_id_segment("cmd_vel"));
        assert!(valid_id_segment("imu-0.cam"));
    }

    #[test]
    fn valid_id_segment_rejects_empty() {
        assert!(!valid_id_segment(""));
    }

    #[test]
    fn valid_id_segment_rejects_slash() {
        // A slash would smuggle the id into adjacent key segments
        // (cross-session/cross-plane leak).
        assert!(!valid_id_segment("s1/command"));
        assert!(!valid_id_segment("a/b"));
    }

    #[test]
    fn valid_id_segment_rejects_wildcards_and_whitespace() {
        for bad in ["*", "**", "s*", "a$b", "a#b", "a?b", "s 1", "s\t1", "s\n1"] {
            assert!(!valid_id_segment(bad), "{bad:?} should be rejected");
        }
    }

    #[test]
    #[should_panic]
    fn session_key_builder_rejects_wildcard_id_in_debug() {
        // The builder `debug_assert!`s the id; a wildcard id must trip it
        // (tests run with debug assertions on).
        let _ = Keys::default().sensor("../*");
    }
}
