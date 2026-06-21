//! Transport-neutral **bus** abstraction: RPC via a *queryable* on a key
//! expression, streaming via *pub/sub* on per-session keys. Peers address data
//! (`{realm}/**`), not server addresses — location-transparent, many-to-many.
//!
//! `ncp-core` ships a synchronous `Bus` trait plus an in-process `LocalBus`
//! (deterministic, dependency-free — for tests and co-process use) and the
//! generic `NcpBusClient` / `NcpBusServer` that carry NCP over any `Bus`. The
//! Zenoh binding lives in the `ncp-zenoh` crate. Mirrors `backend/neurocontrol/
//! bus.py`.

use crate::keys::Keys;
use std::sync::{Arc, Mutex};

/// Answers an RPC: request bytes → reply bytes.
pub type QueryHandler = Arc<dyn Fn(&[u8]) -> Vec<u8> + Send + Sync>;
/// Receives a published sample: (key, payload).
pub type SubCallback = Arc<dyn Fn(&str, &[u8]) + Send + Sync>;

/// Minimal zenoh-style key match: exact, `prefix/**`, or `*` single-segment.
pub fn key_matches(pattern: &str, key: &str) -> bool {
    if pattern == key {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return key == prefix || key.starts_with(&format!("{prefix}/"));
    }
    let pp: Vec<&str> = pattern.split('/').collect();
    let kp: Vec<&str> = key.split('/').collect();
    if pp.len() != kp.len() {
        return false;
    }
    pp.iter().zip(kp.iter()).all(|(p, k)| *p == "*" || p == k)
}

/// A data-centric bus: queryable RPC + pub/sub streaming.
pub trait Bus: Send + Sync {
    /// Register an RPC responder on `key`.
    fn declare_queryable(&self, key: &str, handler: QueryHandler);
    /// Query `key` and return the first reply payload.
    fn query(&self, key: &str, payload: &[u8]) -> Result<Vec<u8>, BusError>;
    /// Subscribe to samples on `key` (may contain `*`/`**`).
    fn declare_subscriber(&self, key: &str, callback: SubCallback);
    /// Publish `payload` on `key`.
    fn put(&self, key: &str, payload: &[u8]) -> Result<(), BusError>;
    /// Tear down.
    fn close(&self) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusError(pub String);

impl std::fmt::Display for BusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for BusError {}

/// In-process bus: models queryable RPC + pub/sub so the decoupled binding is
/// testable and usable same-process (co-process SITL).
#[derive(Clone, Default)]
pub struct LocalBus {
    queryables: Arc<Mutex<Vec<(String, QueryHandler)>>>,
    subs: Arc<Mutex<Vec<(String, SubCallback)>>>,
}

impl LocalBus {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Bus for LocalBus {
    fn declare_queryable(&self, key: &str, handler: QueryHandler) {
        self.queryables
            .lock()
            .unwrap()
            .push((key.to_string(), handler));
    }

    fn query(&self, key: &str, payload: &[u8]) -> Result<Vec<u8>, BusError> {
        let handler = {
            let qs = self.queryables.lock().unwrap();
            qs.iter()
                .find(|(pat, _)| key_matches(pat, key))
                .map(|(_, h)| h.clone())
        };
        match handler {
            Some(h) => Ok(h(payload)),
            None => Err(BusError(format!("no queryable answers {key:?}"))),
        }
    }

    fn declare_subscriber(&self, key: &str, callback: SubCallback) {
        self.subs.lock().unwrap().push((key.to_string(), callback));
    }

    fn put(&self, key: &str, payload: &[u8]) -> Result<(), BusError> {
        let matched: Vec<SubCallback> = {
            let subs = self.subs.lock().unwrap();
            subs.iter()
                .filter(|(pat, _)| key_matches(pat, key))
                .map(|(_, cb)| cb.clone())
                .collect()
        };
        for cb in matched {
            cb(key, payload);
        }
        Ok(())
    }
}

/// NCP client over a `Bus` — talks only to the bus, never a server address.
pub struct NcpBusClient<B: Bus> {
    pub bus: B,
    pub keys: Keys,
}

impl<B: Bus> NcpBusClient<B> {
    pub fn new(bus: B, keys: Keys) -> Self {
        Self { bus, keys }
    }

    /// Send an NCP RPC message (already-serialized JSON bytes) and return the
    /// reply JSON bytes.
    pub fn request(&self, message: &[u8]) -> Result<Vec<u8>, BusError> {
        self.bus.query(&self.keys.rpc(), message)
    }

    /// Subscribe to the observation stream for a session; `callback` gets raw
    /// JSON payloads.
    pub fn subscribe_observations(&self, session_id: &str, callback: SubCallback) {
        self.bus
            .declare_subscriber(&self.keys.observation(session_id), callback);
    }

    /// Subscribe to the command (action) plane — what a plant does to receive
    /// `CommandFrame`s.
    pub fn subscribe_commands(&self, session_id: &str, callback: SubCallback) {
        self.bus
            .declare_subscriber(&self.keys.command(session_id), callback);
    }

    /// Publish a `SensorFrame` (perception plane) — what a plant does each tick.
    pub fn put_sensor(&self, session_id: &str, payload: &[u8]) -> Result<(), BusError> {
        self.bus.put(&self.keys.sensor(session_id), payload)
    }
}

/// Serve NCP RPC over a `Bus`: a queryable answers Open/Step/Run/Close by
/// delegating to `handler` (e.g. the NCP gateway's `handler` forwards to a
/// backend's `SessionService.handle_json`).
pub struct NcpBusServer<B: Bus> {
    pub bus: B,
    pub keys: Keys,
}

impl<B: Bus> NcpBusServer<B> {
    pub fn new(bus: B, keys: Keys) -> Self {
        Self { bus, keys }
    }

    /// Register the RPC queryable. `handler` maps request JSON bytes → reply
    /// JSON bytes.
    pub fn serve_rpc(&self, handler: QueryHandler) {
        self.bus.declare_queryable(&self.keys.rpc(), handler);
    }

    /// Publish an observation frame (JSON bytes) on a session's observation key.
    pub fn publish_observation(&self, session_id: &str, payload: &[u8]) -> Result<(), BusError> {
        self.bus.put(&self.keys.observation(session_id), payload)
    }

    /// Publish a command frame (JSON bytes) on a session's action plane.
    pub fn publish_command(&self, session_id: &str, payload: &[u8]) -> Result<(), BusError> {
        self.bus.put(&self.keys.command(session_id), payload)
    }

    /// Subscribe to the sensor (perception) plane for a session.
    pub fn subscribe_sensors(&self, session_id: &str, callback: SubCallback) {
        self.bus
            .declare_subscriber(&self.keys.sensor(session_id), callback);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_matching() {
        assert!(key_matches("a/b", "a/b"));
        assert!(key_matches("a/**", "a"));
        assert!(key_matches("a/**", "a/b/c"));
        assert!(key_matches("a/*/c", "a/b/c"));
        assert!(!key_matches("a/*/c", "a/b/d"));
        assert!(!key_matches("a/b", "a/b/c"));
    }

    #[test]
    fn multi_uav_varying_entities_route_with_no_crosstalk() {
        use crate::keys::Keys;
        let bus = LocalBus::new();
        let k = Keys::default();

        // A controller subscribes to each UAV's whole sensor set (any count) via
        // the per-UAV sensor wildcard; the sample key identifies which sensor.
        let sensors = Arc::new(Mutex::new(Vec::<String>::new()));
        for uav in ["uav1", "uav2", "uav3"] {
            let sink = sensors.clone();
            bus.declare_subscriber(
                &k.sensor_glob(uav),
                Arc::new(move |key: &str, _p: &[u8]| sink.lock().unwrap().push(key.to_string())),
            );
        }
        // Plant-side per-actuator subscribers (varying counts per UAV).
        let cmds = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        for (uav, act) in [("uav1", "cmd_vel"), ("uav2", "gimbal"), ("uav3", "rotor2")] {
            let sink = cmds.clone();
            let label = format!("{uav}/{act}");
            bus.declare_subscriber(
                &k.command_named(uav, act),
                Arc::new(move |_key: &str, p: &[u8]| {
                    sink.lock()
                        .unwrap()
                        .push((label.clone(), String::from_utf8_lossy(p).into_owned()))
                }),
            );
        }

        // Varying sensor counts: uav1=3, uav2=1, uav3=2.
        for (uav, name) in [
            ("uav1", "imu"),
            ("uav1", "cam"),
            ("uav1", "lidar"),
            ("uav2", "imu"),
            ("uav3", "imu"),
            ("uav3", "gps"),
        ] {
            bus.put(&k.sensor_named(uav, name), b"x").unwrap();
        }
        // Commands to specific actuators (rotor0 has no subscriber).
        bus.put(&k.command_named("uav3", "rotor2"), b"R2").unwrap();
        bus.put(&k.command_named("uav1", "cmd_vel"), b"V1").unwrap();
        bus.put(&k.command_named("uav3", "rotor0"), b"R0").unwrap();

        let s = sensors.lock().unwrap();
        let count = |u: &str| {
            s.iter()
                .filter(|key| key.contains(&format!("/session/{u}/")))
                .count()
        };
        assert_eq!(
            (count("uav1"), count("uav2"), count("uav3")),
            (3, 1, 2),
            "each UAV's sensor-glob receives exactly its own varying sensor set"
        );

        let c = cmds.lock().unwrap();
        assert!(c.iter().any(|(l, v)| l == "uav3/rotor2" && v == "R2"));
        assert!(c.iter().any(|(l, v)| l == "uav1/cmd_vel" && v == "V1"));
        assert!(
            !c.iter().any(|(_, v)| v == "R0"),
            "rotor0 has no subscriber -> not delivered (no crosstalk)"
        );
    }

    #[test]
    fn local_bus_rpc_and_pubsub() {
        let bus = LocalBus::new();
        bus.declare_queryable(
            "ncp/rpc",
            Arc::new(|p: &[u8]| {
                let mut v = b"echo:".to_vec();
                v.extend_from_slice(p);
                v
            }),
        );
        let reply = bus.query("ncp/rpc", b"hi").unwrap();
        assert_eq!(reply, b"echo:hi");

        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen2 = seen.clone();
        bus.declare_subscriber(
            "ncp/session/s1/**",
            Arc::new(move |_k: &str, p: &[u8]| {
                seen2
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(p).into_owned());
            }),
        );
        bus.put("ncp/session/s1/observation", b"obs").unwrap();
        bus.put("ncp/session/s2/observation", b"nope").unwrap();
        assert_eq!(&*seen.lock().unwrap(), &vec!["obs".to_string()]);
    }
}
