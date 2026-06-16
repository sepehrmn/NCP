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

/// Key-expression builders for a given realm.
#[derive(Clone, Debug)]
pub struct Keys {
    pub realm: String,
}

impl Default for Keys {
    fn default() -> Self {
        Self { realm: DEFAULT_REALM.to_string() }
    }
}

impl Keys {
    pub fn new(realm: impl Into<String>) -> Self {
        Self { realm: realm.into() }
    }

    /// Control-plane RPC queryable (Open/Step/Run/Close).
    pub fn rpc(&self) -> String {
        format!("{}/rpc", self.realm)
    }

    fn session(&self, id: &str) -> String {
        format!("{}/session/{}", self.realm, id)
    }

    /// Perception plane for a session (all sensors): `…/session/{id}/sensor`.
    pub fn sensor(&self, id: &str) -> String {
        format!("{}/sensor", self.session(id))
    }

    /// One named sensor on the perception plane: `…/session/{id}/sensor/{name}`.
    pub fn sensor_named(&self, id: &str, name: &str) -> String {
        format!("{}/sensor/{}", self.session(id), name)
    }

    /// Action plane for a session (all actuators): `…/session/{id}/command`.
    pub fn command(&self, id: &str) -> String {
        format!("{}/command", self.session(id))
    }

    /// One named actuator on the action plane: `…/session/{id}/command/{name}`.
    pub fn command_named(&self, id: &str, name: &str) -> String {
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
        assert_eq!(k.sensor_named("s1", "imu"), "engram/ncp/session/s1/sensor/imu");
        assert_eq!(k.command_named("s1", "cmd_vel"), "engram/ncp/session/s1/command/cmd_vel");
        assert_eq!(k.observation("s1"), "engram/ncp/session/s1/observation");
        assert_eq!(k.session_glob("s1"), "engram/ncp/session/s1/**");
    }
}
