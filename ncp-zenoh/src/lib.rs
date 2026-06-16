//! # ncp-zenoh — the recommended decoupled NCP transport
//!
//! Carries NCP over a **data-centric** Zenoh bus: RPC is a *queryable* on
//! `{realm}/rpc`; the perception, action and observation **data planes** are
//! *pub/sub* on per-session keys (see [`ncp_core::keys`]). Peers address data,
//! not server addresses — location-transparent, many-to-many, and the medium
//! crebain already speaks. Observers (e.g. pid_vla) attach to the data-plane
//! keys read-only with zero changes to the control path.
//!
//! Each plane gets the QoS its job needs (see [`Plane`]):
//! - **perception** — Best-Effort + CongestionControl=DROP (conflate to latest);
//! - **action** — express + DROP + RealTime priority (lowest-latency setpoint),
//!   safety-gated by the sender;
//! - **control/observation** — reliable.
//!
//! Async API (native to Zenoh; all NCP consumers run on tokio). The in-process
//! [`ncp_core::Bus`] / [`ncp_core::LocalBus`] remain for tests and co-process use.

use ncp_core::keys::Keys;
use std::sync::Arc;
use zenoh::qos::{CongestionControl, Priority};
use zenoh::{Config, Session};

/// Re-export so consumers can configure Zenoh without depending on `zenoh`.
pub use zenoh::Config as ZenohConfig;

/// A transport plane with its QoS profile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plane {
    /// Sensors → Engram. Lossy-OK: drop the stale frame.
    Perception,
    /// Engram → actuators. Lowest-latency, express, safety-critical.
    Action,
    /// Lifecycle RPC replies / observation broadcast. Reliable.
    Control,
}

impl Plane {
    fn congestion(self) -> CongestionControl {
        match self {
            // Drop-oldest on the wire for high-rate / latency-critical streams.
            Plane::Perception | Plane::Action => CongestionControl::Drop,
            Plane::Control => CongestionControl::Block,
        }
    }
    fn priority(self) -> Priority {
        match self {
            Plane::Action => Priority::RealTime,
            Plane::Perception => Priority::DataHigh,
            Plane::Control => Priority::Data,
        }
    }
    fn express(self) -> bool {
        // Kill batching for the latency-critical action setpoint.
        matches!(self, Plane::Action)
    }
}

#[derive(Debug)]
pub struct ZenohError(pub String);
impl std::fmt::Display for ZenohError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for ZenohError {}
type Result<T> = std::result::Result<T, ZenohError>;

fn err<E: std::fmt::Display>(ctx: &str) -> impl Fn(E) -> ZenohError + '_ {
    move |e| ZenohError(format!("{ctx}: {e}"))
}

/// An NCP-aware Zenoh session. Wraps a [`zenoh::Session`] with the NCP key scheme
/// and per-plane QoS.
#[derive(Clone)]
pub struct ZenohBus {
    session: Arc<Session>,
    keys: Keys,
    // Retain subscriber handles for the session lifetime — a dropped Zenoh
    // Subscriber undeclares its subscription, so callbacks would stop firing.
    subs: Arc<std::sync::Mutex<Vec<zenoh::pubsub::Subscriber<()>>>>,
}

impl ZenohBus {
    /// Open with the default Zenoh config and realm.
    pub async fn open() -> Result<Self> {
        Self::with_config(Config::default(), Keys::default()).await
    }

    /// Open with the default Zenoh config and an explicit realm.
    pub async fn open_realm(keys: Keys) -> Result<Self> {
        Self::with_config(Config::default(), keys).await
    }

    /// Open with an explicit config and realm.
    pub async fn with_config(config: Config, keys: Keys) -> Result<Self> {
        let session = zenoh::open(config).await.map_err(err("zenoh open"))?;
        Ok(Self {
            session: Arc::new(session),
            keys,
            subs: Arc::new(std::sync::Mutex::new(Vec::new())),
        })
    }

    /// Wrap an already-open session (so a host app, e.g. crebain, can share one
    /// Zenoh session across ROS traffic and NCP).
    pub fn from_session(session: Arc<Session>, keys: Keys) -> Self {
        Self { session, keys, subs: Arc::new(std::sync::Mutex::new(Vec::new())) }
    }

    pub fn keys(&self) -> &Keys {
        &self.keys
    }
    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }

    // ───────────────────────── client side ─────────────────────────

    /// Control-plane RPC: send a serialized NCP message, return the reply bytes.
    pub async fn request(&self, message: &[u8]) -> Result<Vec<u8>> {
        let replies = self
            .session
            .get(self.keys.rpc())
            .payload(message)
            .await
            .map_err(err("zenoh get"))?;
        while let Ok(reply) = replies.recv_async().await {
            if let Ok(sample) = reply.result() {
                return Ok(sample.payload().to_bytes().to_vec());
            }
        }
        Err(ZenohError(format!("no reply for {}", self.keys.rpc())))
    }

    /// Publish a `SensorFrame` (perception plane) for a session.
    pub async fn put_sensor(&self, session_id: &str, payload: &[u8]) -> Result<()> {
        self.put(&self.keys.sensor(session_id), payload, Plane::Perception).await
    }

    /// Subscribe to the command (action) plane — the plant receives `CommandFrame`s.
    pub async fn subscribe_commands<F>(&self, session_id: &str, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        self.subscribe(&self.keys.command(session_id), callback).await
    }

    /// Subscribe to the observation plane (the free read-only observer tap).
    pub async fn subscribe_observations<F>(&self, session_id: &str, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        self.subscribe(&self.keys.observation(session_id), callback).await
    }

    /// Subscribe to every plane of a session (observer/diagnostic tap).
    pub async fn subscribe_session<F>(&self, session_id: &str, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        self.subscribe(&self.keys.session_glob(session_id), callback).await
    }

    // ───────────────────────── server side ─────────────────────────

    /// Serve the control-plane RPC queryable. `handler` maps request JSON bytes →
    /// reply JSON bytes (in the Engram gateway it forwards to Python
    /// `SessionService.handle_json`). Runs until the returned task is dropped.
    pub async fn serve_rpc<F>(&self, handler: F) -> Result<()>
    where
        F: Fn(Vec<u8>) -> Vec<u8> + Send + Sync + 'static,
    {
        let queryable = self
            .session
            .declare_queryable(self.keys.rpc())
            .await
            .map_err(err("declare queryable"))?;
        let handler = Arc::new(handler);
        tokio::spawn(async move {
            while let Ok(query) = queryable.recv_async().await {
                let req = query
                    .payload()
                    .map(|p| p.to_bytes().to_vec())
                    .unwrap_or_default();
                let reply = handler(req);
                let ke = query.key_expr().clone();
                let _ = query.reply(ke, reply).await;
            }
        });
        Ok(())
    }

    /// Publish an observation frame (JSON bytes) on a session's observation key.
    pub async fn publish_observation(&self, session_id: &str, payload: &[u8]) -> Result<()> {
        self.put(&self.keys.observation(session_id), payload, Plane::Control).await
    }

    /// Publish a command frame on a session's action plane (safety-gated upstream).
    pub async fn publish_command(&self, session_id: &str, payload: &[u8]) -> Result<()> {
        self.put(&self.keys.command(session_id), payload, Plane::Action).await
    }

    /// Subscribe to the sensor (perception) plane for a session.
    pub async fn subscribe_sensors<F>(&self, session_id: &str, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        self.subscribe(&self.keys.sensor(session_id), callback).await
    }

    // ───────────────────────── primitives ─────────────────────────

    /// Publish on `key` with the QoS of `plane`.
    pub async fn put(&self, key: &str, payload: &[u8], plane: Plane) -> Result<()> {
        self.session
            .put(key, payload.to_vec())
            .congestion_control(plane.congestion())
            .priority(plane.priority())
            .express(plane.express())
            .await
            .map_err(err("zenoh put"))
    }

    /// Subscribe to `key` (may contain `*`/`**`); `callback` gets `(key, bytes)`.
    pub async fn subscribe<F>(&self, key: &str, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        let callback = Arc::new(callback);
        let sub = self
            .session
            .declare_subscriber(key.to_string())
            .callback(move |sample| {
                let key = sample.key_expr().as_str().to_string();
                let payload = sample.payload().to_bytes().to_vec();
                callback(key, payload);
            })
            .await
            .map_err(err("declare subscriber"))?;
        // Keep the handle alive (dropping it undeclares the subscription).
        self.subs.lock().unwrap().push(sub);
        Ok(())
    }

    pub fn close(&self) {
        // Best-effort; the session closes when the last Arc is dropped.
        let _ = self.session.clone();
    }
}

/// A [`ncp_core::ControlTransport`] backed by Zenoh — the **controller side** of
/// the streaming closed loop. It subscribes to the perception plane
/// (`…/session/{id}/sensor`), keeping the latest `SensorFrame`, and publishes
/// `CommandFrame`s to the safety-gated action plane (`…/command`). Drop it into a
/// `ncp_core::NeuroControlLoop` to run a spiking or reflex controller over Zenoh
/// **streaming** — no per-tick RPC round trip. Construct within a tokio runtime.
pub struct ZenohControlTransport {
    bus: ZenohBus,
    session_id: String,
    latest: Arc<std::sync::Mutex<Option<ncp_core::SensorFrame>>>,
    handle: tokio::runtime::Handle,
}

impl ZenohControlTransport {
    pub async fn new(bus: ZenohBus, session_id: impl Into<String>) -> Result<Self> {
        let session_id = session_id.into();
        let latest: Arc<std::sync::Mutex<Option<ncp_core::SensorFrame>>> =
            Arc::new(std::sync::Mutex::new(None));
        let sink = latest.clone();
        bus.subscribe_sensors(&session_id, move |_key, bytes| {
            if let Ok(sf) = serde_json::from_slice::<ncp_core::SensorFrame>(&bytes) {
                *sink.lock().unwrap() = Some(sf);
            }
        })
        .await?;
        Ok(Self { bus, session_id, latest, handle: tokio::runtime::Handle::current() })
    }
}

impl ncp_core::ControlTransport for ZenohControlTransport {
    fn send_command(&self, command: &ncp_core::CommandFrame) {
        let Ok(bytes) = serde_json::to_vec(command) else { return };
        let bus = self.bus.clone();
        let session_id = self.session_id.clone();
        // Fire-and-forget put on the action plane (express + DROP + RealTime QoS).
        self.handle.spawn(async move {
            let _ = bus.publish_command(&session_id, &bytes).await;
        });
    }

    fn latest_sensor(&self) -> Option<ncp_core::SensorFrame> {
        self.latest.lock().unwrap().clone()
    }
}

/// Convenience: a typed NCP client over Zenoh.
pub struct ZenohNcpClient {
    bus: ZenohBus,
}

impl ZenohNcpClient {
    pub fn new(bus: ZenohBus) -> Self {
        Self { bus }
    }

    /// Open a session; returns the parsed `SessionOpened`.
    pub async fn open(&self, msg: &ncp_core::OpenSession) -> Result<ncp_core::SessionOpened> {
        self.rpc(msg).await
    }

    /// Step a session; returns the parsed `ObservationFrame`.
    pub async fn step(&self, msg: &ncp_core::StepRequest) -> Result<ncp_core::ObservationFrame> {
        self.rpc(msg).await
    }

    /// Run a session for a duration; returns the parsed `ObservationFrame`.
    pub async fn run(&self, msg: &ncp_core::RunRequest) -> Result<ncp_core::ObservationFrame> {
        self.rpc(msg).await
    }

    /// Close a session.
    pub async fn close(&self, msg: &ncp_core::CloseSession) -> Result<ncp_core::SessionClosed> {
        self.rpc(msg).await
    }

    async fn rpc<Req, Resp>(&self, msg: &Req) -> Result<Resp>
    where
        Req: serde::Serialize,
        Resp: serde::de::DeserializeOwned,
    {
        let req = serde_json::to_vec(msg).map_err(err("serialize request"))?;
        let reply = self.bus.request(&req).await?;
        // Surface an error frame as an Err rather than a parse failure.
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&reply) {
            if ncp_core::message_kind(&v) == Some("error") {
                return Err(ZenohError(format!(
                    "NCP error: {}",
                    v.get("error").and_then(|e| e.as_str()).unwrap_or("unknown")
                )));
            }
        }
        serde_json::from_slice(&reply).map_err(err("parse reply"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_qos_profiles() {
        assert_eq!(Plane::Action.congestion(), CongestionControl::Drop);
        assert_eq!(Plane::Action.priority(), Priority::RealTime);
        assert!(Plane::Action.express());
        assert_eq!(Plane::Control.congestion(), CongestionControl::Block);
        assert!(!Plane::Perception.express());
    }
}
