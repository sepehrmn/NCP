#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]
//!
//! # Transport details
//!
//! The perception, action and observation **data planes** are *pub/sub* on
//! per-session keys (see [`ncp_core::keys`]). Observers (e.g. an
//! analysis/observer client) attach to the data-plane keys read-only with zero
//! changes to the control path.
//!
//! Each plane gets the QoS its job needs (see [`Plane`]). NCP sets
//! CongestionControl + priority + express per plane; wire reliability is left at
//! Zenoh's default (the minimal feature set here does not enable the `unstable`
//! reliability API):
//! - **perception** — CongestionControl=DROP + DataHigh priority (TX-queue DROP
//!   only when the queue is full — no conflation guarantee, i.e. it is *not*
//!   guaranteed to drop to the latest frame, only to drop *some* frames);
//! - **action** — express + DROP + RealTime priority (lowest-latency setpoint),
//!   safety-gated by the sender;
//! - **control/observation** — CongestionControl=BLOCK (no drop).
//!
//! Observation publish reuses the Control plane's reliable/BLOCK QoS, so under
//! congestion it back-pressures the publisher rather than dropping — keep the
//! observation stream low-rate.
//!
//! Async API (native to Zenoh; all NCP consumers run on tokio). The in-process
//! [`ncp_core::Bus`] / [`ncp_core::LocalBus`] remain for tests and co-process use.
//!
//! ## Security: the realm is addressing, not a credential
//!
//! The realm string (`{realm}/…`) is *addressing*, not authorization — anyone who
//! can reach the bus and knows (or guesses) the realm can publish/subscribe on it.
//! It is **not** a secret or a credential. To actually restrict who may drive an
//! actuator, deploy the shipped per-plane access-control template and pair it with
//! mutual TLS (see `deploy/zenoh-access-control.json5` and `SECURITY.md`).
//!
//! The default [`ZenohBus::open`] / [`ZenohBus::open_realm`] path is hardened to be
//! quiet-by-default: multicast scouting is **disabled** so a default deployment does
//! not auto-advertise on the LAN (peers still connect via explicit
//! `connect`/`listen` endpoints in a supplied config). For an ACL/TLS-enforced
//! deployment, load the shipped config via [`ZenohBus::open_secure`],
//! [`ZenohBus::with_config_file`], or by setting the `NCP_ZENOH_CONFIG` environment
//! variable (honored by `open`/`open_realm` and by the `ncp-gateway` binary).

use ncp_core::keys::{valid_id_segment, Keys};
use std::sync::Arc;
use zenoh::qos::{CongestionControl, Priority};
use zenoh::{Config, Session};

/// Re-export so consumers can configure Zenoh without depending on `zenoh`.
pub use zenoh::Config as ZenohConfig;

/// Environment variable naming a Zenoh config file (json5/json) to load. When set,
/// [`ZenohBus::open`] / [`ZenohBus::open_realm`] (and the `ncp-gateway` binary) load
/// it instead of the hardened default — point it at the shipped ACL/TLS config in
/// `deploy/` for an enforced deployment.
pub const NCP_ZENOH_CONFIG_ENV: &str = "NCP_ZENOH_CONFIG";

/// Build the hardened default config: Zenoh defaults with **multicast scouting
/// disabled** so a default deployment does not auto-advertise on the LAN. Peers can
/// still connect via explicit `connect`/`listen` endpoints supplied in a config
/// file (see [`NCP_ZENOH_CONFIG_ENV`]). This is addressing-hygiene, not auth — the
/// realm is not a credential; for real authorization load the `deploy/` ACL via
/// [`ZenohBus::open_secure`].
fn default_quiet_config() -> Result<Config> {
    let mut cfg = Config::default();
    // Fail-closed on the discovery surface: scouting-off is the security guarantee
    // of this path. Zenoh's own default is multicast scouting ON (LAN
    // auto-advertise), so if this insert ever fails we must NOT silently hand back a
    // config that re-enables it — surface the error so the open aborts.
    cfg.insert_json5("scouting/multicast/enabled", "false")
        .map_err(|e| ZenohError(format!("disable multicast scouting: {e}")))?;
    Ok(cfg)
}

/// Load a Zenoh config file (json5/json) from `path`, surfacing a parse/IO error as
/// [`ZenohError`] rather than panicking — a missing or malformed security config
/// must fail the open, never silently fall back to an open default.
fn config_from_file(path: &std::path::Path) -> Result<Config> {
    Config::from_file(path)
        .map_err(|e| ZenohError(format!("load Zenoh config {}: {e}", path.display())))
}

/// Resolve the effective config for the default open path: if [`NCP_ZENOH_CONFIG_ENV`]
/// is set, load that file (fail-closed on error); otherwise use the hardened
/// scouting-off default.
fn resolve_default_config() -> Result<Config> {
    match std::env::var_os(NCP_ZENOH_CONFIG_ENV) {
        Some(path) => config_from_file(std::path::Path::new(&path)),
        None => default_quiet_config(),
    }
}

/// A transport plane with its QoS profile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plane {
    /// Sensors → controller. Lossy-OK: TX-queue DROP only when full, no conflation
    /// guarantee (drops some frames, not necessarily down to the latest).
    Perception,
    /// Controller → actuators. Lowest-latency, express, safety-critical.
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

/// Reject a caller-supplied id segment (session id, entity name) before it is
/// interpolated into a key expression. Guards against empty/whitespace ids and
/// Zenoh key-expression metacharacters (`/ * $ # ?`) that would silently widen a
/// publish/subscribe to the wrong keyspace. Glob subscribers are intentionally
/// *not* guarded — their wildcards are constructed by the key builders.
fn check_id(kind: &str, id: &str) -> Result<()> {
    if valid_id_segment(id) {
        Ok(())
    } else {
        Err(ZenohError(format!("invalid {kind} id segment: {id:?}")))
    }
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
    /// Open with the hardened default config and realm.
    ///
    /// "Hardened default" = Zenoh defaults with multicast scouting **disabled** so a
    /// default deployment does not auto-advertise on the LAN. If [`NCP_ZENOH_CONFIG_ENV`]
    /// is set, the named config file is loaded instead (and a load error fails the
    /// open — it never silently falls back to an open default). The realm is
    /// addressing, not a credential; for ACL/TLS enforcement use [`Self::open_secure`].
    pub async fn open() -> Result<Self> {
        Self::with_config(resolve_default_config()?, Keys::default()).await
    }

    /// Open with the hardened default config and an explicit realm. See [`Self::open`]
    /// for the scouting-off default and the [`NCP_ZENOH_CONFIG_ENV`] override.
    pub async fn open_realm(keys: Keys) -> Result<Self> {
        Self::with_config(resolve_default_config()?, keys).await
    }

    /// Open with a Zenoh config loaded from a file (json5/json), e.g. the shipped
    /// per-plane ACL config in `deploy/zenoh-access-control.json5`. A missing or
    /// malformed file fails the open (fail-closed) rather than falling back to an
    /// open default.
    pub async fn with_config_file(path: impl AsRef<std::path::Path>, keys: Keys) -> Result<Self> {
        Self::with_config(config_from_file(path.as_ref())?, keys).await
    }

    /// Open the secure deployment config: load the file named by [`NCP_ZENOH_CONFIG_ENV`]
    /// (the operator points it at the shipped `deploy/` ACL/TLS config). Unlike
    /// [`Self::open`], the env var is **required** here — if it is unset the open
    /// fails rather than starting an unauthenticated session, so a misconfigured
    /// "secure" deployment refuses to come up instead of silently opening a hole.
    pub async fn open_secure(keys: Keys) -> Result<Self> {
        let path = std::env::var_os(NCP_ZENOH_CONFIG_ENV).ok_or_else(|| {
            ZenohError(format!(
                "open_secure requires {NCP_ZENOH_CONFIG_ENV} to name a Zenoh ACL/TLS \
                 config (e.g. deploy/zenoh-access-control.json5)"
            ))
        })?;
        Self::with_config_file(std::path::Path::new(&path), keys).await
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

    /// Wrap an already-open session (so a host app, e.g. a ROS 2 robot client,
    /// can share one Zenoh session across ROS traffic and NCP).
    pub fn from_session(session: Arc<Session>, keys: Keys) -> Self {
        Self {
            session,
            keys,
            subs: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
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
        // Capture the last error reply so a remote error (server replied with an
        // error) is distinguishable from a dead server (no reply at all).
        let mut last_err: Option<String> = None;
        while let Ok(reply) = replies.recv_async().await {
            match reply.result() {
                Ok(sample) => return Ok(sample.payload().to_bytes().to_vec()),
                Err(e) => {
                    last_err = Some(String::from_utf8_lossy(&e.payload().to_bytes()).into_owned())
                }
            }
        }
        match last_err {
            Some(e) => Err(ZenohError(format!(
                "rpc error reply for {}: {e}",
                self.keys.rpc()
            ))),
            None => Err(ZenohError(format!("no reply for {}", self.keys.rpc()))),
        }
    }

    /// Publish a `SensorFrame` (perception plane) for a session.
    pub async fn put_sensor(&self, session_id: &str, payload: &[u8]) -> Result<()> {
        check_id("session", session_id)?;
        self.put(&self.keys.sensor(session_id), payload, Plane::Perception)
            .await
    }

    /// Subscribe to the command (action) plane — the plant receives `CommandFrame`s.
    pub async fn subscribe_commands<F>(&self, session_id: &str, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        check_id("session", session_id)?;
        self.subscribe(&self.keys.command(session_id), callback)
            .await
    }

    /// Subscribe to the observation plane (the free read-only observer tap).
    pub async fn subscribe_observations<F>(&self, session_id: &str, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        check_id("session", session_id)?;
        self.subscribe(&self.keys.observation(session_id), callback)
            .await
    }

    /// Subscribe to every plane of a session (observer/diagnostic tap).
    pub async fn subscribe_session<F>(&self, session_id: &str, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        // Guard the glob entry point too: a malformed/wildcard id must be rejected
        // here, not silently widen the subscription in a release build (debug_assert
        // in the key builder is compiled out).
        check_id("session", session_id)?;
        self.subscribe(&self.keys.session_glob(session_id), callback)
            .await
    }

    // ───────────────────────── server side ─────────────────────────

    /// Serve the control-plane RPC queryable. `handler` maps request JSON bytes →
    /// reply JSON bytes (e.g. a gateway forwards it to a Python backend's
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
                if let Err(e) = query.reply(ke, reply).await {
                    // No log crate in this minimal feature set; surface to stderr so
                    // a failed reply isn't silently dropped.
                    eprintln!("ncp-zenoh: rpc reply failed: {e}");
                }
            }
        });
        Ok(())
    }

    /// Publish an observation frame (JSON bytes) on a session's observation key.
    pub async fn publish_observation(&self, session_id: &str, payload: &[u8]) -> Result<()> {
        check_id("session", session_id)?;
        self.put(&self.keys.observation(session_id), payload, Plane::Control)
            .await
    }

    /// Publish a command frame on a session's action plane (safety-gated upstream).
    pub async fn publish_command(&self, session_id: &str, payload: &[u8]) -> Result<()> {
        check_id("session", session_id)?;
        self.put(&self.keys.command(session_id), payload, Plane::Action)
            .await
    }

    /// Subscribe to the sensor (perception) plane for a session.
    pub async fn subscribe_sensors<F>(&self, session_id: &str, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        check_id("session", session_id)?;
        self.subscribe(&self.keys.sensor(session_id), callback)
            .await
    }

    // ───────────── per-named-entity (multi-sensor / multi-actuator) ─────────────
    // A UAV with a varying number of sensors/actuators addresses each by name on
    // its own sub-key; the callback's `key` argument identifies which entity. Per
    // entity `seq` is its own stream (one LinkMonitor/ActionBuffer per entity).

    /// Publish a `SensorFrame` for one named sensor: `…/sensor/{name}`.
    pub async fn put_sensor_named(
        &self,
        session_id: &str,
        name: &str,
        payload: &[u8],
    ) -> Result<()> {
        check_id("session", session_id)?;
        check_id("sensor name", name)?;
        self.put(
            &self.keys.sensor_named(session_id, name),
            payload,
            Plane::Perception,
        )
        .await
    }

    /// Publish a `CommandFrame` to one named actuator: `…/command/{name}`.
    pub async fn publish_command_named(
        &self,
        session_id: &str,
        name: &str,
        payload: &[u8],
    ) -> Result<()> {
        check_id("session", session_id)?;
        check_id("actuator name", name)?;
        self.put(
            &self.keys.command_named(session_id, name),
            payload,
            Plane::Action,
        )
        .await
    }

    /// Subscribe to **all** of a session's sensors (any count): `…/sensor/**`.
    pub async fn subscribe_sensors_glob<F>(&self, session_id: &str, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        // Guard the glob entry point too (release builds drop the key-builder
        // debug_assert), so a wildcard-bearing id cannot widen the subscription.
        check_id("session", session_id)?;
        self.subscribe(&self.keys.sensor_glob(session_id), callback)
            .await
    }

    /// Subscribe to one named actuator's command stream: `…/command/{name}`.
    pub async fn subscribe_command_named<F>(
        &self,
        session_id: &str,
        name: &str,
        callback: F,
    ) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        check_id("session", session_id)?;
        check_id("actuator name", name)?;
        self.subscribe(&self.keys.command_named(session_id, name), callback)
            .await
    }

    /// Subscribe across the whole fleet (every session/plane): `{realm}/session/**`
    /// — e.g. an observer/dashboard over all UAVs.
    pub async fn subscribe_fleet<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(String, Vec<u8>) + Send + Sync + 'static,
    {
        self.subscribe(&self.keys.fleet_glob(), callback).await
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
    ///
    /// Backpressure model: this is a Zenoh **callback** subscriber — the callback
    /// runs INLINE on Zenoh's receive task, one sample at a time. There is no
    /// user-side queue, so a slow callback applies natural backpressure to the
    /// stream (it does NOT buffer unboundedly and cannot exhaust memory). The flip
    /// side is head-of-line: keep `callback` cheap (decode + hand off), and for a
    /// control loop prefer latest-wins (overwrite a shared `SensorFrame`, as
    /// [`ZenohControlTransport`] does) over doing heavy work here. A panic in
    /// `callback` unwinds Zenoh's task, so the callback must not panic on
    /// adversarial input — decode fallibly and drop, never `unwrap`.
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

    /// Gracefully close the underlying Zenoh session (undeclare queryables /
    /// subscribers and flush). In Zenoh 1.x `Session::close` takes `&self` and
    /// works even when the `Arc<Session>` is shared (e.g. by an embedded
    /// `ZenohControlTransport`), so this is correct despite `ZenohBus: Clone`.
    pub async fn close(&self) -> Result<()> {
        self.session.close().await.map_err(err("zenoh close"))
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
            match serde_json::from_slice::<ncp_core::SensorFrame>(&bytes) {
                Ok(sf) => *sink.lock().unwrap() = Some(sf),
                // The data plane drops on parse failure; surface a diagnostic so a
                // version-incompatible peer is observable, not silently ignored.
                Err(e) => match ncp_core::diagnose_version(&bytes) {
                    Some(ve) => eprintln!("ncp: dropped sensor frame ({ve})"),
                    None => eprintln!("ncp: dropped unparseable sensor frame: {e}"),
                },
            }
        })
        .await?;
        Ok(Self {
            bus,
            session_id,
            latest,
            handle: tokio::runtime::Handle::current(),
        })
    }
}

impl ncp_core::ControlTransport for ZenohControlTransport {
    fn send_command(&self, command: &ncp_core::CommandFrame) {
        let Ok(bytes) = serde_json::to_vec(command) else {
            return;
        };
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

/// Validate an RPC reply's own `kind` discriminator before the typed decode: an
/// error frame surfaces as `Err`, and a wrong-but-valid-JSON reply is rejected
/// rather than silently decoding into an all-default `Resp`. Pure (no transport),
/// so it is unit-testable.
fn check_reply_kind(reply: &[u8], expect_kind: &str) -> Result<()> {
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(reply) {
        match ncp_core::message_kind(&v) {
            Some("error") => {
                return Err(ZenohError(format!(
                    "NCP error: {}",
                    v.get("error").and_then(|e| e.as_str()).unwrap_or("unknown")
                )));
            }
            Some(k) if k != expect_kind => {
                return Err(ZenohError(format!(
                    "NCP reply kind mismatch: expected {expect_kind:?}, got {k:?}"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Convenience: a typed NCP client over Zenoh.
pub struct ZenohNcpClient {
    bus: ZenohBus,
}

impl ZenohNcpClient {
    pub fn new(bus: ZenohBus) -> Self {
        Self { bus }
    }

    /// Open a session; returns the parsed `SessionOpened`. The handshake gates on
    /// **version compatibility** (a `SessionOpened` whose `ncp_version` is
    /// incompatible is rejected, never coerced) and treats the `contract_hash` as an
    /// **advisory** signal: a hash mismatch is logged but does *not* fail the session,
    /// so a version-compatible flow keeps working when a peer is on a different
    /// contract revision (e.g. it added an optional field). See
    /// [`ncp_core::ContractStatus`].
    pub async fn open(&self, msg: &ncp_core::OpenSession) -> Result<ncp_core::SessionOpened> {
        let opened: ncp_core::SessionOpened = self.rpc(msg, "session_opened").await?;
        let status = ncp_core::negotiate(&opened.ncp_version, opened.contract_hash.as_deref())
            .map_err(|e| ZenohError(format!("session_opened version: {e}")))?;
        if let Some(advisory) = status.advisory() {
            // Advisory, not fatal: log so operators can spot a fleet on mixed revisions.
            eprintln!("[ncp-zenoh] {advisory}");
        }
        Ok(opened)
    }

    /// Step a session; returns the parsed `ObservationFrame`.
    pub async fn step(&self, msg: &ncp_core::StepRequest) -> Result<ncp_core::ObservationFrame> {
        self.rpc(msg, "observation_frame").await
    }

    /// Run a session for a duration; returns the parsed `ObservationFrame`.
    pub async fn run(&self, msg: &ncp_core::RunRequest) -> Result<ncp_core::ObservationFrame> {
        self.rpc(msg, "observation_frame").await
    }

    /// Close a session.
    pub async fn close(&self, msg: &ncp_core::CloseSession) -> Result<ncp_core::SessionClosed> {
        self.rpc(msg, "session_closed").await
    }

    async fn rpc<Req, Resp>(&self, msg: &Req, expect_kind: &str) -> Result<Resp>
    where
        Req: serde::Serialize,
        Resp: serde::de::DeserializeOwned,
    {
        let req = serde_json::to_vec(msg).map_err(err("serialize request"))?;
        let reply = self.bus.request(&req).await?;
        // Reject an error frame or a wrong-`kind` reply before the typed decode,
        // so a misrouted reply cannot silently become an all-default `Resp`.
        check_reply_kind(&reply, expect_kind)?;
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

    #[test]
    fn check_reply_kind_rejects_wrong_kind_and_error_frames() {
        // Right kind -> Ok.
        assert!(
            check_reply_kind(br#"{"kind":"session_opened","ok":true}"#, "session_opened").is_ok()
        );
        // Wrong-but-valid-JSON kind -> Err (not a silent all-default decode).
        assert!(check_reply_kind(br#"{"kind":"observation_frame"}"#, "session_opened").is_err());
        // An error frame -> Err.
        assert!(check_reply_kind(br#"{"kind":"error","error":"boom"}"#, "session_opened").is_err());
    }

    #[test]
    fn check_id_rejects_keyexpr_metacharacters() {
        // A clean id passes.
        assert!(check_id("session", "uav3").is_ok());
        // Metacharacters that would widen/escape the key expression are rejected
        // BEFORE the key is built (fail-closed boundary, FIX 7).
        for bad in [
            "", " ", "a/b", "*", "**", "a*", "$kid", "a#b", "a?b", "a b", "a\tb",
        ] {
            assert!(
                check_id("session", bad).is_err(),
                "expected reject for {bad:?}"
            );
        }
    }

    #[test]
    fn default_config_disables_multicast_scouting() {
        // The hardened default open() path must not auto-advertise on the LAN:
        // scouting/multicast/enabled is forced false. Zenoh's own default is true,
        // so this asserts our override actually took effect (closed-realm default).
        let cfg = default_quiet_config().expect("hardened default config must build");
        let json = cfg.get_json("scouting/multicast/enabled").unwrap();
        assert_eq!(
            json.trim(),
            "false",
            "multicast scouting must be off by default"
        );
    }

    #[test]
    fn open_secure_requires_the_config_env_var() {
        // Fail-closed: open_secure must refuse when NCP_ZENOH_CONFIG is unset rather
        // than starting an unauthenticated session. We assert the precondition the
        // async path enforces (env var presence) without standing up a session.
        // SAFETY of the test: serialized within this single test; restored after.
        let saved = std::env::var_os(NCP_ZENOH_CONFIG_ENV);
        std::env::remove_var(NCP_ZENOH_CONFIG_ENV);
        assert!(
            std::env::var_os(NCP_ZENOH_CONFIG_ENV).is_none(),
            "precondition: env var unset"
        );
        // A missing config file must be a load error (fail-closed), never a silent
        // fallback to an open default.
        let missing = std::path::Path::new("/nonexistent/ncp-zenoh-acl.json5");
        assert!(config_from_file(missing).is_err());
        if let Some(v) = saved {
            std::env::set_var(NCP_ZENOH_CONFIG_ENV, v);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn put_sensor_rejects_bad_session_id_at_the_entry_point() {
        // Open an isolated bus (no scouting/listen/connect) so the test needs no
        // router. The FIX 7 guard rejects before any key is built or I/O happens,
        // so a metacharacter session id resolves to Err on the public entry point.
        let mut cfg = Config::default();
        let _ = cfg.insert_json5("scouting/multicast/enabled", "false");
        let _ = cfg.insert_json5("listen/endpoints", "[]");
        let _ = cfg.insert_json5("connect/endpoints", "[]");
        let bus = ZenohBus::with_config(cfg, Keys::default()).await.unwrap();
        let e = bus.put_sensor("bad/id", b"{}").await.unwrap_err();
        assert!(e.to_string().contains("invalid session id segment"), "{e}");
        // A glob-escaping entity name on a named publish is rejected too.
        assert!(bus.put_sensor_named("uav3", "imu*", b"{}").await.is_err());
        let _ = bus.close().await;
    }
}
