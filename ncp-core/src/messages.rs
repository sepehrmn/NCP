//! NCP wire messages — the normative payload contract.
//!
//! Every type here serializes to **exactly** the JSON that the Python reference
//! (`backend/neurocontrol/{protocol,session}.py`, Pydantic v2) and the protobuf
//! IDL (`ncp.proto`) produce, so the Rust, Python and TypeScript peers are wire
//! compatible over any transport. In particular:
//!
//! - enums serialize as their string *values* (`"V_m"`, `"spike_times"`, …),
//! - every message carries a `kind` discriminator and an `ncp_version`,
//! - `Option::None` serializes as JSON `null` (Pydantic includes nulls),
//! - unknown fields are **ignored** on deserialize (forward compatible within a
//!   major version — consumers ignore unknown fields).
//!
//! Construct messages with `..Default::default()` (or the `new` helpers) so the
//! `kind`/`ncp_version` defaults are filled in.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Protocol version (semver). Receivers check the **major** only.
pub const NCP_VERSION: &str = "0.1";

fn ncp_version() -> String {
    NCP_VERSION.to_string()
}

/// A JSON object map (`{string: T}`); `BTreeMap` for deterministic ordering.
pub type Map<T> = BTreeMap<String, T>;

// ───────────────────────── enums ─────────────────────────

/// What to record off a population/neuron/synapse.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Observable {
    #[serde(rename = "spikes")]
    Spikes,
    #[default]
    #[serde(rename = "V_m")]
    Vm,
    #[serde(rename = "rate")]
    Rate,
    #[serde(rename = "weight")]
    Weight,
}

/// How a stimulus drives a target.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum StimulusKind {
    #[default]
    #[serde(rename = "current_pA")]
    CurrentPa,
    #[serde(rename = "rate_hz")]
    RateHz,
    #[serde(rename = "spike_times")]
    SpikeTimes,
    #[serde(rename = "weight_set")]
    WeightSet,
}

/// What kind of network reference `NetworkRef.ref` is.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum NetworkRefKind {
    #[serde(rename = "handle")]
    Handle,
    #[default]
    #[serde(rename = "builtin")]
    Builtin,
    #[serde(rename = "model_id")]
    ModelId,
    #[serde(rename = "spec")]
    Spec,
}

/// Stream vs batch simulation.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum SimMode {
    #[default]
    #[serde(rename = "stream")]
    Stream,
    #[serde(rename = "batch")]
    Batch,
}

/// Controller mode (the safety-critical action authority lives here).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Mode {
    #[serde(rename = "init")]
    Init,
    #[default]
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "hold")]
    Hold,
    #[serde(rename = "estop")]
    Estop,
}

/// Hierarchical entity role for addressing sensors/actuators.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum EntityRole {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "actor")]
    Actor,
    #[default]
    #[serde(rename = "sensor")]
    Sensor,
    #[serde(rename = "actuator")]
    Actuator,
}

/// Channel arity (carries the vec semantics so the envelope stays generic).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ChannelKind {
    #[default]
    #[serde(rename = "scalar")]
    Scalar,
    #[serde(rename = "vec3")]
    Vec3,
    #[serde(rename = "quat")]
    Quat,
    #[serde(rename = "array")]
    Array,
}

/// Who a peer is in the closed-loop handshake.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Role {
    #[default]
    #[serde(rename = "controller")]
    Controller,
    #[serde(rename = "plant")]
    Plant,
}

// ───────────────────────── primitives ─────────────────────────

/// A channel sample: a flat list of floats plus an optional unit string. Width
/// carries the semantics (1=scalar, 3=vec3, 4=quat, N=array).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ChannelValue {
    #[serde(default)]
    pub data: Vec<f64>,
    #[serde(default)]
    pub unit: Option<String>,
}

impl ChannelValue {
    pub fn scalar(value: f64, unit: Option<&str>) -> Self {
        Self { data: vec![value], unit: unit.map(str::to_string) }
    }
    pub fn vec3(x: f64, y: f64, z: f64, unit: Option<&str>) -> Self {
        Self { data: vec![x, y, z], unit: unit.map(str::to_string) }
    }
}

// ───────────────────────── entity addressing ─────────────────────────

/// A hierarchical client-side entity address, e.g. `crebain/uav3/sensor/cam0`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct EntityRef {
    pub path: String,
    pub role: EntityRole,
    pub meta: Map<String>,
}

/// Binds a client entity to a stimulus or record port.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct EntityBinding {
    pub entity: EntityRef,
    pub port: String,
    /// `"stimulus"` | `"record"`.
    pub direction: String,
}

impl Default for EntityBinding {
    fn default() -> Self {
        Self { entity: EntityRef::default(), port: String::new(), direction: "stimulus".into() }
    }
}

// ───────────────────────── network / sim config ─────────────────────────

/// What to simulate.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct NetworkRef {
    pub kind: NetworkRefKind,
    /// builtin model name, or a `compiled_module_id` (kind=handle). `ref` is a
    /// Rust keyword, so the field is `ref_` and renamed on the wire.
    #[serde(rename = "ref")]
    pub ref_: String,
    /// kind=handle: which registered model to create if the handle has >1.
    pub model_name: Option<String>,
    pub population_sizes: Map<i64>,
    pub params: Map<f64>,
}

/// Integration / streaming configuration.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct SimConfig {
    pub dt_ms: f64,
    pub chunk_ms: f64,
    pub seed: Option<i64>,
    pub mode: SimMode,
    pub duration_ms: Option<f64>,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self { dt_ms: 0.1, chunk_ms: 10.0, seed: None, mode: SimMode::Stream, duration_ms: None }
    }
}

// ───────────────────────── record / stimulus specs ─────────────────────────

/// One recording: client `port` name ← `observable` of `target` population.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct RecordTarget {
    pub port: String,
    pub target: String,
    pub observable: Observable,
    pub ids: Vec<i64>,
    pub cadence_ms: f64,
}

impl Default for RecordTarget {
    fn default() -> Self {
        Self {
            port: String::new(),
            target: String::new(),
            observable: Observable::Vm,
            ids: Vec::new(),
            cadence_ms: 1.0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct RecordSpec {
    pub targets: Vec<RecordTarget>,
}

/// One stimulus input port.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct StimulusTarget {
    pub port: String,
    pub target: String,
    pub kind: StimulusKind,
    pub ids: Vec<i64>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct StimulusSpec {
    pub targets: Vec<StimulusTarget>,
}

// ───────────────────────── provenance ─────────────────────────

/// Scientific-boundary discriminators carried on every opened session. Returned
/// data is a **raw simulation output of a specified model**, never a validated
/// reproduction: `calibrated_posterior=false`, `is_simulation_output=true`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct SimProvenance {
    pub network_ref: String,
    pub backend: String,
    pub seed: Option<i64>,
    pub calibrated_posterior: bool,
    pub is_simulation_output: bool,
    pub advisory_only: bool,
    pub note: Option<String>,
}

impl Default for SimProvenance {
    fn default() -> Self {
        Self {
            network_ref: String::new(),
            backend: String::new(),
            seed: None,
            calibrated_posterior: false,
            is_simulation_output: true,
            advisory_only: true,
            note: None,
        }
    }
}

// ───────────────────────── simulation-service messages ─────────────────────────

/// Request a simulation: declare what to record and what to stimulate.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct OpenSession {
    pub ncp_version: String,
    pub kind: String,
    pub session_id: String,
    pub network: NetworkRef,
    pub record: RecordSpec,
    pub stimulus: StimulusSpec,
    pub sim: SimConfig,
    pub bindings: Vec<EntityBinding>,
}

impl Default for OpenSession {
    fn default() -> Self {
        Self {
            ncp_version: ncp_version(),
            kind: "open_session".into(),
            session_id: String::new(),
            network: NetworkRef::default(),
            record: RecordSpec::default(),
            stimulus: StimulusSpec::default(),
            sim: SimConfig::default(),
            bindings: Vec::new(),
        }
    }
}

/// Ack of `open_session` with resolved sizes and provenance.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct SessionOpened {
    pub ncp_version: String,
    pub kind: String,
    pub session_id: String,
    pub ok: bool,
    pub backend: String,
    pub resolved: Map<i64>,
    pub provenance: Option<SimProvenance>,
    pub error: Option<String>,
}

impl Default for SessionOpened {
    fn default() -> Self {
        Self {
            ncp_version: ncp_version(),
            kind: "session_opened".into(),
            session_id: String::new(),
            ok: true,
            backend: "mock".into(),
            resolved: Map::new(),
            provenance: None,
            error: None,
        }
    }
}

/// The values to inject this step (keyed by stimulus port).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct StimulusFrame {
    pub ncp_version: String,
    pub kind: String,
    pub session_id: String,
    pub t: f64,
    pub values: Map<ChannelValue>,
}

impl Default for StimulusFrame {
    fn default() -> Self {
        Self {
            ncp_version: ncp_version(),
            kind: "stimulus_frame".into(),
            session_id: String::new(),
            t: 0.0,
            values: Map::new(),
        }
    }
}

/// Advance one chunk; optional stimulus; returns an `ObservationFrame`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct StepRequest {
    pub ncp_version: String,
    pub kind: String,
    pub session_id: String,
    pub advance_ms: Option<f64>,
    pub stimulus: Option<StimulusFrame>,
}

impl Default for StepRequest {
    fn default() -> Self {
        Self {
            ncp_version: ncp_version(),
            kind: "step_request".into(),
            session_id: String::new(),
            advance_ms: None,
            stimulus: None,
        }
    }
}

/// Batch: advance `duration_ms` holding a stimulus; returns an `ObservationFrame`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct RunRequest {
    pub ncp_version: String,
    pub kind: String,
    pub session_id: String,
    pub duration_ms: f64,
    pub stimulus: Option<StimulusFrame>,
}

impl Default for RunRequest {
    fn default() -> Self {
        Self {
            ncp_version: ncp_version(),
            kind: "run_request".into(),
            session_id: String::new(),
            duration_ms: 0.0,
            stimulus: None,
        }
    }
}

/// Recorded data for one record port. `times`+`values` are parallel for analog;
/// `times`+`senders` are parallel for spikes.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct Observation {
    pub port: String,
    pub target: String,
    pub observable: Observable,
    pub times: Vec<f64>,
    pub values: Vec<f64>,
    pub senders: Vec<i64>,
    pub unit: Option<String>,
}

/// The returned neural data, keyed by record port.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct ObservationFrame {
    pub ncp_version: String,
    pub kind: String,
    pub session_id: String,
    /// Echoes the driving `SensorFrame.seq` when published inside a closed loop,
    /// so a split-plane observer can align `(V,L,D,A)` on `seq` (not arrival
    /// time). `0` in the pure pull/sim-service path (no controller seq).
    pub seq: i64,
    pub t: f64,
    pub sim_time_ms: f64,
    pub records: Map<Observation>,
    pub calibrated_posterior: bool,
    pub is_simulation_output: bool,
}

impl Default for ObservationFrame {
    fn default() -> Self {
        Self {
            ncp_version: ncp_version(),
            kind: "observation_frame".into(),
            session_id: String::new(),
            seq: 0,
            t: 0.0,
            sim_time_ms: 0.0,
            records: Map::new(),
            calibrated_posterior: false,
            is_simulation_output: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct CloseSession {
    pub ncp_version: String,
    pub kind: String,
    pub session_id: String,
}

impl Default for CloseSession {
    fn default() -> Self {
        Self { ncp_version: ncp_version(), kind: "close_session".into(), session_id: String::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct SessionClosed {
    pub ncp_version: String,
    pub kind: String,
    pub session_id: String,
    pub ok: bool,
}

impl Default for SessionClosed {
    fn default() -> Self {
        Self {
            ncp_version: ncp_version(),
            kind: "session_closed".into(),
            session_id: String::new(),
            ok: true,
        }
    }
}

// ───────────────────────── closed-loop control messages ─────────────────────────

/// Declares a named channel a controller produces or consumes.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct ChannelSpec {
    pub name: String,
    pub kind: ChannelKind,
    pub unit: Option<String>,
    pub size: Option<i64>,
    pub optional: bool,
    pub description: Option<String>,
}

impl Default for ChannelSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: ChannelKind::Scalar,
            unit: None,
            size: None,
            optional: true,
            description: None,
        }
    }
}

/// Bounds the action plane. Only the command path enforces these.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct SafetyLimits {
    pub max_speed_mps: Option<f64>,
    pub max_tilt_rad: Option<f64>,
    pub geofence_radius_m: Option<f64>,
    pub command_timeout_ms: f64,
}

impl Default for SafetyLimits {
    fn default() -> Self {
        Self {
            max_speed_mps: None,
            max_tilt_rad: None,
            geofence_radius_m: None,
            command_timeout_ms: 500.0,
        }
    }
}

/// Handshake: who the controller is and what it speaks.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct Capabilities {
    pub ncp_version: String,
    pub kind: String,
    pub controller_id: String,
    pub role: Role,
    pub control_rate_hz: f64,
    pub sensor_channels: Vec<ChannelSpec>,
    pub command_channels: Vec<ChannelSpec>,
    pub codec_id: Option<String>,
    pub safety: SafetyLimits,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            ncp_version: ncp_version(),
            kind: "capabilities".into(),
            controller_id: String::new(),
            role: Role::Controller,
            control_rate_hz: 20.0,
            sensor_channels: Vec::new(),
            command_channels: Vec::new(),
            codec_id: None,
            safety: SafetyLimits::default(),
        }
    }
}

/// Plant → controller: the latest sensed state. Carries `seq`/`t` so a command
/// can be stamped with the sensor it was computed from (the correspondence the
/// split perception/action planes must preserve — join on `seq`, not arrival).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct SensorFrame {
    pub ncp_version: String,
    pub kind: String,
    pub seq: i64,
    pub t: f64,
    pub frame_id: String,
    pub channels: Map<ChannelValue>,
}

impl Default for SensorFrame {
    fn default() -> Self {
        Self {
            ncp_version: ncp_version(),
            kind: "sensor_frame".into(),
            seq: 0,
            t: 0.0,
            frame_id: "world".into(),
            channels: Map::new(),
        }
    }
}

/// Controller → plant: the proposed actuation, with `mode`/`ttl_ms` safety
/// metadata. `seq` should echo the originating `SensorFrame.seq`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct CommandFrame {
    pub ncp_version: String,
    pub kind: String,
    pub seq: i64,
    pub t: f64,
    pub frame_id: String,
    pub mode: Mode,
    pub ttl_ms: f64,
    pub channels: Map<ChannelValue>,
}

impl Default for CommandFrame {
    fn default() -> Self {
        Self {
            ncp_version: ncp_version(),
            kind: "command_frame".into(),
            seq: 0,
            t: 0.0,
            frame_id: "world".into(),
            mode: Mode::Active,
            ttl_ms: 200.0,
            channels: Map::new(),
        }
    }
}

/// Controller → plant / telemetry: loop health and mode.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct ControlStatus {
    pub ncp_version: String,
    pub kind: String,
    pub seq: i64,
    pub t: f64,
    pub mode: Mode,
    pub sim_time_ms: f64,
    pub loop_latency_ms: f64,
    pub safety_ok: bool,
    pub note: Option<String>,
}

impl Default for ControlStatus {
    fn default() -> Self {
        Self {
            ncp_version: ncp_version(),
            kind: "control_status".into(),
            seq: 0,
            t: 0.0,
            mode: Mode::Init,
            sim_time_ms: 0.0,
            loop_latency_ms: 0.0,
            safety_ok: true,
            note: None,
        }
    }
}

// ───────────────────────── version guard ─────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NcpVersionError(pub String);

impl std::fmt::Display for NcpVersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for NcpVersionError {}

/// Major-compatible? Same major is forward compatible (consumers ignore unknown
/// fields). On a major mismatch, `Err` when `strict` else `Ok(false)`.
pub fn check_version(version: &str, strict: bool) -> Result<bool, NcpVersionError> {
    let parse_major = |s: &str| -> Result<u64, NcpVersionError> {
        s.split('.')
            .next()
            .and_then(|m| m.parse::<u64>().ok())
            .ok_or_else(|| NcpVersionError(format!("unparseable ncp_version {s:?}")))
    };
    let got = parse_major(version)?;
    let want = parse_major(NCP_VERSION)?;
    if got != want {
        if strict {
            return Err(NcpVersionError(format!(
                "NCP major mismatch: got {version}, want {NCP_VERSION}"
            )));
        }
        return Ok(false);
    }
    Ok(true)
}

/// Read the `kind` discriminator off any NCP JSON (for client reply dispatch).
pub fn message_kind(json: &serde_json::Value) -> Option<&str> {
    json.get("kind").and_then(|v| v.as_str())
}
