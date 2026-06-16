//! Transport-neutral **bus** abstraction: RPC via a *queryable* on a key
//! expression, streaming via *pub/sub* on per-session keys. Peers address data
//! (`engram/ncp/**`), not server addresses — location-transparent, many-to-many.
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
        self.queryables.lock().unwrap().push((key.to_string(), handler));
    }

    fn query(&self, key: &str, payload: &[u8]) -> Result<Vec<u8>, BusError> {
        let handler = {
            let qs = self.queryables.lock().unwrap();
            qs.iter().find(|(pat, _)| key_matches(pat, key)).map(|(_, h)| h.clone())
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
            subs.iter().filter(|(pat, _)| key_matches(pat, key)).map(|(_, cb)| cb.clone()).collect()
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
        self.bus.declare_subscriber(&self.keys.observation(session_id), callback);
    }

    /// Subscribe to the command (action) plane — what a plant does to receive
    /// `CommandFrame`s.
    pub fn subscribe_commands(&self, session_id: &str, callback: SubCallback) {
        self.bus.declare_subscriber(&self.keys.command(session_id), callback);
    }

    /// Publish a `SensorFrame` (perception plane) — what a plant does each tick.
    pub fn put_sensor(&self, session_id: &str, payload: &[u8]) -> Result<(), BusError> {
        self.bus.put(&self.keys.sensor(session_id), payload)
    }
}

/// Serve NCP RPC over a `Bus`: a queryable answers Open/Step/Run/Close by
/// delegating to `handler` (in the Engram gateway, `handler` forwards to the
/// Python `SessionService.handle_json`).
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
        self.bus.declare_subscriber(&self.keys.sensor(session_id), callback);
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
    fn local_bus_rpc_and_pubsub() {
        let bus = LocalBus::new();
        bus.declare_queryable("engram/ncp/rpc", Arc::new(|p: &[u8]| {
            let mut v = b"echo:".to_vec();
            v.extend_from_slice(p);
            v
        }));
        let reply = bus.query("engram/ncp/rpc", b"hi").unwrap();
        assert_eq!(reply, b"echo:hi");

        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen2 = seen.clone();
        bus.declare_subscriber("engram/ncp/session/s1/**", Arc::new(move |_k: &str, p: &[u8]| {
            seen2.lock().unwrap().push(String::from_utf8_lossy(p).into_owned());
        }));
        bus.put("engram/ncp/session/s1/observation", b"obs").unwrap();
        bus.put("engram/ncp/session/s2/observation", b"nope").unwrap();
        assert_eq!(&*seen.lock().unwrap(), &vec!["obs".to_string()]);
    }
}
