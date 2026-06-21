//! NCP wire messages — the normative payload contract.
//!
//! Every type here serializes to **semantically-equivalent JSON** to what the
//! Python reference (`backend/neurocontrol/{protocol,session}.py`, Pydantic v2)
//! and the protobuf IDL (`ncp.proto`) produce, so the Rust, Python and
//! TypeScript peers are wire compatible over any transport (map key order may
//! differ between encoders). In particular:
//!
//! - enums serialize as their string *values* (`"V_m"`, `"spike_times"`, …),
//! - every message carries a `kind` discriminator and an `ncp_version`,
//! - `Option::None` serializes as JSON `null` (Pydantic includes nulls),
//! - unknown fields are **ignored** on deserialize. This is forward compatible
//!   only within a compatible version: while the wire is pre-1.0 (`0.x`) the
//!   minor is breaking, so [`check_version`] requires an exact `(major, minor)`
//!   match; once `>=1.0` the major alone gates compatibility.
//!
//! Construct messages with `..Default::default()` (or the `new` helpers) so the
//! `kind`/`ncp_version` defaults are filled in.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Protocol version (semver). While pre-1.0 (`0.x`) receivers check the full
/// `(major, minor)`; once `>=1.0` they check the **major** only. See
/// [`check_version`].
pub const NCP_VERSION: &str = "0.4";

fn ncp_version() -> String {
    NCP_VERSION.to_string()
}

/// A JSON object map (`{string: T}`); `BTreeMap` for deterministic ordering.
pub type Map<T> = BTreeMap<String, T>;

// ───────────────────────── enums ─────────────────────────

/// What to record off a population/neuron/synapse.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// Binary / multi-state neurons: discrete state via spin_detector, not V_m. (#10)
    #[serde(rename = "binary_state")]
    BinaryState,
}

/// How a stimulus drives a target.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// Continuous-rate injection for rate-based neurons (rate connections /
    /// step_rate_generator); rate models cannot receive spikes. (#10)
    #[serde(rename = "rate_inject")]
    RateInject,
}

/// What kind of network reference `NetworkRef.ref` is.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SimMode {
    #[default]
    #[serde(rename = "stream")]
    Stream,
    #[serde(rename = "batch")]
    Batch,
}

/// Controller mode (the safety-critical action authority lives here).
// Deserialize is hand-written below (fail-safe: an unknown mode string -> Hold,
// never errors the whole frame). Serialize stays derived; the rename attrs apply.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

impl<'de> Deserialize<'de> for Mode {
    /// Fail-safe: an unrecognized `mode` string deserializes to `Hold` rather than
    /// erroring the whole `CommandFrame` — a peer that sends a mode this build does
    /// not recognize must neither actuate nor have its frame dropped. (An ABSENT
    /// `mode` is handled separately by the field-level `default_command_mode`.)
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "init" => Mode::Init,
            "active" => Mode::Active,
            "hold" => Mode::Hold,
            "estop" => Mode::Estop,
            _ => Mode::Hold,
        })
    }
}

/// Hierarchical entity role for addressing sensors/actuators.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChannelValue {
    #[serde(default)]
    pub data: Vec<f64>,
    #[serde(default)]
    pub unit: Option<String>,
}

impl ChannelValue {
    pub fn scalar(value: f64, unit: Option<&str>) -> Self {
        Self {
            data: vec![value],
            unit: unit.map(str::to_string),
        }
    }
    pub fn vec3(x: f64, y: f64, z: f64, unit: Option<&str>) -> Self {
        Self {
            data: vec![x, y, z],
            unit: unit.map(str::to_string),
        }
    }
}

// ───────────────────────── entity addressing ─────────────────────────

/// A hierarchical client-side entity address, e.g. `uav1/sensor/cam0`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct EntityRef {
    pub path: String,
    pub role: EntityRole,
    pub meta: Map<String>,
}

/// Binds a client entity to a stimulus or record port.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct EntityBinding {
    pub entity: EntityRef,
    pub port: String,
    /// `"stimulus"` | `"record"`.
    pub direction: String,
}

impl Default for EntityBinding {
    fn default() -> Self {
        Self {
            entity: EntityRef::default(),
            port: String::new(),
            direction: "stimulus".into(),
        }
    }
}

// ───────────────────────── network / sim config ─────────────────────────

/// What to simulate.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
        Self {
            dt_ms: 0.1,
            chunk_ms: 10.0,
            seed: None,
            mode: SimMode::Stream,
            duration_ms: None,
        }
    }
}

// ───────────────────────── record / stimulus specs ─────────────────────────

/// One recording: client `port` name ← `observable` of `target` population.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct RecordTarget {
    pub port: String,
    pub target: String,
    pub observable: Observable,
    pub ids: Vec<i64>,
    pub cadence_ms: f64,
    /// Generic named multimeter recordables (model-specific: e.g. `g_ex`/`g_in`
    /// for conductance models, `w` for aeif, `rate` for rate models). Empty =
    /// just `observable`. Resolved via NEST multimeter `record_from`. (#10)
    pub recordables: Vec<String>,
}

impl Default for RecordTarget {
    fn default() -> Self {
        Self {
            port: String::new(),
            target: String::new(),
            observable: Observable::Vm,
            ids: Vec::new(),
            cadence_ms: 1.0,
            recordables: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct RecordSpec {
    pub targets: Vec<RecordTarget>,
}

/// One stimulus input port.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct StimulusTarget {
    pub port: String,
    pub target: String,
    pub kind: StimulusKind,
    pub ids: Vec<i64>,
    /// Named stimulus parameters beyond the scalar value, e.g. siegert_neuron's
    /// diffusion_connection `drift_factor` / `diffusion_factor`. (#10)
    pub params: Map<f64>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// Caller's [`CONTRACT_HASH`], carried in the handshake as an **advisory**
    /// identity signal (see [`ContractStatus`]): a mismatch is logged, not rejected —
    /// `ncp_version` is the hard compatibility gate. Defaults to our own hash so
    /// every session advertises it; `None` (serialized `null`) = not advertised.
    pub contract_hash: Option<String>,
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
            contract_hash: Some(CONTRACT_HASH.to_string()),
        }
    }
}

/// Ack of `open_session` with resolved sizes and provenance.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// Server's [`CONTRACT_HASH`] — the reply half of the handshake (see
    /// [`OpenSession::contract_hash`]). A client treats a hash difference as an
    /// **advisory** ([`ContractStatus::Mismatch`], logged not rejected); the version
    /// is the hard gate. `None` (serialized `null`) = not advertised.
    pub contract_hash: Option<String>,
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
            contract_hash: Some(CONTRACT_HASH.to_string()),
        }
    }
}

/// The values to inject this step (keyed by stimulus port).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Observation {
    pub port: String,
    pub target: String,
    pub observable: Observable,
    pub times: Vec<f64>,
    pub values: Vec<f64>,
    pub senders: Vec<i64>,
    pub unit: Option<String>,
    /// Which named recordable this series carries (e.g. `g_ex`, `w`) when a port
    /// records more than the primary `observable`; `None` = the `observable`. (#10)
    pub recordable: Option<String>,
}

/// The returned neural data, keyed by record port.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct CloseSession {
    pub ncp_version: String,
    pub kind: String,
    pub session_id: String,
}

impl Default for CloseSession {
    fn default() -> Self {
        Self {
            ncp_version: ncp_version(),
            kind: "close_session".into(),
            session_id: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

/// Bounds the action plane. `max_speed_mps`, `geofence_radius_m` and
/// `command_timeout_ms` are enforced by the action-plane safety governor;
/// `max_tilt_rad` is advisory metadata and is **not** enforced in this layer
/// (no command-path clamp consumes it yet).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct CommandFrame {
    pub ncp_version: String,
    pub kind: String,
    pub seq: i64,
    pub t: f64,
    pub frame_id: String,
    // Fail-safe: a wire frame that OMITS `mode` deserializes to HOLD, never to an
    // actuating mode — an untrusted/partial CommandFrame must not silently drive.
    // (Programmatic `CommandFrame::default()` is unchanged; the controller always
    // sets `mode` explicitly.)
    #[serde(default = "default_command_mode")]
    pub mode: Mode,
    pub ttl_ms: f64,
    pub channels: Map<ChannelValue>,
    /// Packetized predictive control: future setpoints. `channels` is tick 0;
    /// `horizon[i]` applies at tick i+1, spaced `horizon_dt_ms` apart. The
    /// actuator replays these through dropouts (see `ActionBuffer`), bounded by
    /// `ttl_ms`. Empty = legacy single-step command. Backward compatible: a
    /// consumer that ignores `horizon` still reads `channels` (tick 0).
    pub horizon: Vec<Map<ChannelValue>>,
    pub horizon_dt_ms: Option<f64>,
}

/// Wire default for `CommandFrame.mode`: a frame that omits `mode` is HOLD, never
/// an actuating mode (fail-safe deserialization of an untrusted/partial frame).
fn default_command_mode() -> Mode {
    Mode::Hold
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
            horizon: Vec::new(),
            horizon_dt_ms: None,
        }
    }
}

/// Controller → plant / telemetry: loop health and mode.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

/// Link-health telemetry from the seq-gap / CUSUM monitor (published on the
/// control plane). `burst=true` flags sustained loss — a possible jam — at which
/// point the only sound response is to fail safe, not add redundancy.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct LinkStatus {
    pub ncp_version: String,
    pub kind: String,
    pub session_id: String,
    pub t: f64,
    pub last_seq: i64,
    pub received: i64,
    pub lost: i64,
    pub loss_rate: f64,
    pub burst: bool,
}

impl Default for LinkStatus {
    fn default() -> Self {
        Self {
            ncp_version: ncp_version(),
            kind: "link_status".into(),
            session_id: String::new(),
            t: 0.0,
            last_seq: -1,
            received: 0,
            lost: 0,
            loss_rate: 0.0,
            burst: false,
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

/// Compatible? For a pre-1.0 wire (major == 0) the protocol has no stability
/// guarantee yet, so *both* major and minor must match exactly (0.1 ≠ 0.9). For
/// a stable wire (major >= 1) the major alone decides compatibility (consumers
/// ignore unknown fields within a major). On a mismatch, `Err` when `strict`
/// else `Ok(false)`.
pub fn check_version(version: &str, strict: bool) -> Result<bool, NcpVersionError> {
    let parse_ver = |s: &str| -> Result<(u64, u64), NcpVersionError> {
        let err = || NcpVersionError(format!("unparseable ncp_version {s:?}"));
        // Strict: 1 or 2 dot-separated components, each a base-10 u64 with no
        // trailing junk. A malformed minor ("2.GARBAGE") or extra component
        // ("0.2.x") must REJECT, never silently coerce to 0 — otherwise the
        // fail-closed guard becomes fail-open the moment our own minor is 0.
        let mut parts = s.split('.');
        let major = parts.next().ok_or_else(err)?;
        let major: u64 = major.parse().map_err(|_| err())?;
        let minor: u64 = match parts.next() {
            // Missing minor (e.g. "1") is treated as minor 0...
            None => 0,
            // ...but a PRESENT minor must parse strictly.
            Some(m) => m.parse().map_err(|_| err())?,
        };
        // No third component allowed (semver patch is not part of the wire id).
        if parts.next().is_some() {
            return Err(err());
        }
        Ok((major, minor))
    };
    let (got_major, got_minor) = parse_ver(version)?;
    let (want_major, want_minor) = parse_ver(NCP_VERSION)?;
    // Pre-1.0: minor is breaking, so require an exact (major, minor) match.
    // Stable (>=1.0): major-only compatibility.
    let compatible = if want_major == 0 {
        (got_major, got_minor) == (want_major, want_minor)
    } else {
        got_major == want_major
    };
    if !compatible {
        if strict {
            return Err(NcpVersionError(format!(
                "NCP version mismatch: got {version}, want {NCP_VERSION}"
            )));
        }
        return Ok(false);
    }
    Ok(true)
}

/// FNV-1a (64-bit) hex digest of the **canonicalized** normative wire contract
/// ([`canonical_proto`] of `proto/ncp.proto` — comments and formatting stripped).
/// Peers exchange this alongside `ncp_version` in the control-plane handshake (the
/// `contract_hash` field of [`OpenSession`] / [`SessionOpened`]) and reject a
/// mismatch, so a post-agreement schema mutation (the "rug-pull" failure class) is
/// *detectable* rather than silently coerced. It is recomputed from the actual proto
/// by the `contract_hash_matches_proto` test, so a proto edit that forgets to bump
/// this constant fails CI — but a comment- or whitespace-only edit no longer flips it
/// (the churn the `v0.2.5`/`v0.2.6` releases documented).
///
/// # Why a hardcoded constant (and not computed at runtime)?
///
/// The value is **baked in, not derived at runtime**, and that is deliberate.
///
/// 1. **The proto is not on disk at runtime.** [`contract_hash_of_proto`] reads
///    `proto/ncp.proto` via `CARGO_MANIFEST_DIR`, a path that only exists in the
///    *source tree* at build/test time. A shipped `ncp-core` (binary, the PyO3
///    wheel, the C ABI) has no `.proto` to hash, so the value a running peer
///    advertises must be embedded.
/// 2. **It is a contract *identity*, not a derived quantity.** Hardcoding makes
///    "which wire do I claim to speak" an explicit, greppable, reviewable fact, and
///    makes bumping it a deliberate, visible diff rather than an invisible
///    recompute.
/// 3. **It is the shared cross-language anchor.** The Rust and Python peers
///    (`ncp_core::CONTRACT_HASH` and `backend/neurocontrol/protocol.py::CONTRACT_HASH`)
///    pin the *same* string and each independently recomputes it from its own copy of
///    the proto in a test. The pinned constant is the single value both are checked
///    against — a canonicalization bug in one language is caught by CI instead of
///    silently producing two hashes that reject each other.
/// 4. **Drift is impossible to ship, not merely unlikely.** The
///    `contract_hash_matches_proto` test asserts `contract_hash_of_proto(proto) ==
///    CONTRACT_HASH`, so the constant cannot diverge from the proto it claims to
///    represent without failing CI. It is "hardcoded, but *provably equal* to the
///    computed value."
///
/// The considered alternative is to drop the constant entirely and compute it once at
/// startup from a compile-time-embedded proto:
/// `LazyLock::new(|| contract_hash_of_proto(include_str!(".../ncp.proto").as_bytes()))`.
/// That removes the forgot-to-bump class of errors, but loses the `const`-usability,
/// the greppable/reviewable value, and the "bumping it is a deliberate event"
/// property — and still needs a per-language anchor for cross-language parity. The
/// constant-plus-CI-guard form is kept on purpose. See `VERSIONING.md` (§"Contract
/// hash") for the full rationale and the handshake design.
pub const CONTRACT_HASH: &str = "2cf0763ad61e4f1c";

/// FNV-1a (64-bit) hex digest of `bytes`. Dependency-free (no sha/digest crate),
/// adequate for the contract-pinning integrity-vs-accidental-drift use. It is
/// **not** a cryptographic MAC — adversarial integrity is the transport's job
/// (mTLS); this detects unintended/forgotten contract drift between peers.
pub fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Canonicalize a `.proto` source so the contract hash depends only on the
/// *wire-semantic* definition — the message/field/enum structure — not on
/// comments, formatting, **or non-wire declarations** (`syntax`, `package`,
/// `import`, top-level `option`).
///
/// Protobuf's wire encoding is determined by field numbers, types, and modifiers
/// — **never** by comments, by the `package` namespace, or by file options. So a
/// purely *naming* change (e.g. renaming the package `engram.ncp.v0 → ncp.v0` to
/// decouple the protocol's identity from a consumer) leaves the wire identical and
/// MUST leave [`CONTRACT_HASH`] identical too. This pass therefore:
/// 1. removes `//` line and `/* … */` block comments — respecting string literals
///    so a `//` *inside* a quoted default is preserved,
/// 2. drops top-level non-wire declaration lines (`syntax`/`package`/`import`/
///    `option`), which are codegen/deployment metadata, not wire shape,
/// 3. trims each line and drops blank lines.
///
/// The result is that cosmetic and naming changes are hash-neutral, while any real
/// wire change (add/remove/retype a field, change an enum value) still flips the
/// hash. Dependency-free (no protoc/buf): adequate for the accidental-drift
/// detection this hash targets (adversarial integrity is the transport's job).
pub fn canonical_proto(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(quote) = in_string {
            out.push(c);
            if c == '\\' {
                // Preserve the escaped char verbatim (e.g. \" or \\).
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == quote {
                in_string = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                in_string = Some(c);
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                // Line comment: skip to (but keep) the newline.
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                // Block comment: skip until the closing `*/`.
                chars.next(); // consume '*'
                let mut prev = '\0';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            _ => out.push(c),
        }
    }
    // Normalize whitespace and drop non-wire declaration lines. `syntax`,
    // `package`, `import`, and top-level `option` are codegen/deployment metadata
    // that never affect the wire encoding, so a rename of any of them must NOT
    // change the contract hash.
    let normalized = out
        .lines()
        .map(str::trim_start)
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            !(line.starts_with("syntax")
                || line.starts_with("package ")
                || line.starts_with("import ")
                || line.starts_with("option "))
        })
        .collect::<Vec<_>>()
        .join("\n");
    normalized.into_bytes()
}

/// The contract hash of a `.proto` source: [`fnv1a_hex`] of its
/// [`canonical_proto`] form. This is the value pinned in [`CONTRACT_HASH`] and
/// the function a peer uses to recompute the contract identity from its own copy
/// of the proto, so two peers agree iff their *semantic* contracts agree —
/// independent of comments or formatting.
pub fn contract_hash_of_proto(bytes: &[u8]) -> String {
    fnv1a_hex(&canonical_proto(bytes))
}

/// Outcome of comparing a peer's advertised [`CONTRACT_HASH`] to ours.
///
/// This is **advisory**: a [`ContractStatus::Mismatch`] does *not* fail the
/// handshake. [`NCP_VERSION`] (via [`check_version`]) is the *compatibility* gate —
/// "can we speak the same wire at all"; the contract hash is a finer *identity*
/// signal — "are we on the exact same contract revision". Conflating the two
/// (fail-closed on hash) would break a version-compatible flow the moment one peer
/// added an optional field or renamed a non-wire declaration. So a mismatch is
/// surfaced for logging/telemetry, and the session proceeds. (The hash is not a
/// cryptographic MAC; adversarial integrity is the transport's job — mTLS.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractStatus {
    /// Peer advertised a hash equal to ours — same contract revision.
    Match,
    /// Peer advertised no hash (older/minimal peer). Accepted within a compatible version.
    NotAdvertised,
    /// Peer advertised a *different* hash. Advisory only — log it; the session still opens.
    Mismatch { peer: String },
}

impl ContractStatus {
    /// `true` unless the peer advertised a different hash.
    pub fn is_match(&self) -> bool {
        !matches!(self, ContractStatus::Mismatch { .. })
    }
    /// A human-readable advisory string for a mismatch (for logging), else `None`.
    pub fn advisory(&self) -> Option<String> {
        match self {
            ContractStatus::Mismatch { peer } => Some(format!(
                "NCP contract-hash differs: peer {peer:?}, ours {CONTRACT_HASH:?} — \
                 versions are compatible so the session proceeds, but the peers are on \
                 different contract revisions (advisory)"
            )),
            _ => None,
        }
    }
}

/// Classify a peer-advertised contract hash against ours (advisory; see
/// [`ContractStatus`]). Never fails — `None` = not advertised, `Some(==ours)` =
/// match, `Some(!=ours)` = mismatch.
pub fn contract_status(peer_hash: Option<&str>) -> ContractStatus {
    match peer_hash {
        None => ContractStatus::NotAdvertised,
        Some(h) if h == CONTRACT_HASH => ContractStatus::Match,
        Some(h) => ContractStatus::Mismatch {
            peer: h.to_string(),
        },
    }
}

/// **Strict** contract verification (opt-in): a typed error on hash mismatch.
/// Most callers want [`negotiate`] (advisory). Use this only where a deployment
/// has decided that an exact contract-revision match is mandatory and a mismatch
/// must fail closed (e.g. a safety-certified configuration).
pub fn verify_contract(peer_hash: Option<&str>) -> Result<(), NcpVersionError> {
    match contract_status(peer_hash) {
        ContractStatus::Mismatch { peer } => Err(NcpVersionError(format!(
            "NCP contract-hash mismatch: peer {peer:?}, want {CONTRACT_HASH:?}"
        ))),
        _ => Ok(()),
    }
}

/// Handshake gate a control-plane `open_session` calls. The `(major, minor)`
/// version MUST be compatible (fail-closed [`NcpVersionError`] otherwise) — this is
/// the wire-compatibility gate. The contract hash is returned as an **advisory**
/// [`ContractStatus`] (a mismatch does NOT fail the handshake; the caller logs it).
/// This separation lets additive optional fields and non-wire renames evolve the
/// contract without breaking version-compatible peers.
pub fn negotiate(
    peer_version: &str,
    peer_contract_hash: Option<&str>,
) -> Result<ContractStatus, NcpVersionError> {
    check_version(peer_version, true)?;
    Ok(contract_status(peer_contract_hash))
}

/// Best-effort version diagnostic for a raw inbound frame. If it carries an
/// `ncp_version` incompatible with ours, return the typed error so a receiver can
/// log WHY a frame was dropped — the data plane otherwise drops silently. Returns
/// `None` when the frame is compatible, carries no version, or is unparseable.
pub fn diagnose_version(bytes: &[u8]) -> Option<NcpVersionError> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let ver = v.get("ncp_version")?.as_str()?;
    check_version(ver, true).err()
}

/// Read the `kind` discriminator off any NCP JSON (for client reply dispatch).
pub fn message_kind(json: &serde_json::Value) -> Option<&str> {
    json.get("kind").and_then(|v| v.as_str())
}

/// The schema-`required` field names for a given message `kind` — the validation
/// contract (`validate()` enforces these). The serde types default every field
/// (struct-level `#[serde(default)]`), so this, not the serde derive, is the source
/// of truth for what a peer MUST send; `gen-schemas` injects these into each
/// schema's `required` array. Kinds with no required fields return `[]`; an unknown
/// `kind` returns `None`.
pub fn required_fields(kind: &str) -> Option<&'static [&'static str]> {
    Some(match kind {
        "capabilities" => &["controller_id"],
        "close_session" => &["session_id"],
        "command_frame" => &[],
        "control_status" => &[],
        "link_status" => &[],
        "observation_frame" => &["session_id"],
        "open_session" => &["session_id", "network"],
        "run_request" => &["session_id", "duration_ms"],
        "sensor_frame" => &[],
        "session_closed" => &["session_id"],
        "session_opened" => &["session_id"],
        "step_request" => &["session_id"],
        "stimulus_frame" => &["session_id"],
        _ => return None,
    })
}

/// Validation failure: either the JSON is structurally unusable, the `kind` is
/// unknown, or a schema-required field is absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError(pub String);

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for ValidationError {}

/// Validate raw NCP JSON against the wire contract for its `kind`.
///
/// Every message struct is `#[serde(default)]` with no `deny_unknown_fields`, so
/// a typed `serde_json::from_*` round-trip alone is *not* honest: it silently
/// fills in defaults for required-but-missing fields (e.g. a `step_request`
/// with no `session_id` deserializes to an empty session id rather than
/// failing). This function closes that gap by checking the `kind`'s
/// schema-`required` array (the same arrays `tests/conformance.rs` reads from
/// `ncp/schemas/`) **before** trusting the typed value:
///
///   - the payload must be a JSON object,
///   - it must carry a known `kind`,
///   - every schema-required field for that `kind` must be present.
///
/// Unknown extra fields are still accepted (forward compatibility within a
/// compatible version), so this stays wire-safe.
pub fn validate(json: &serde_json::Value) -> Result<(), ValidationError> {
    let obj = json
        .as_object()
        .ok_or_else(|| ValidationError("NCP message is not a JSON object".into()))?;
    let kind = message_kind(json)
        .ok_or_else(|| ValidationError("NCP message has no string `kind`".into()))?;
    let required = required_fields(kind)
        .ok_or_else(|| ValidationError(format!("unknown NCP message kind {kind:?}")))?;
    for field in required {
        if !obj.contains_key(*field) {
            return Err(ValidationError(format!(
                "{kind}: required field {field:?} is missing"
            )));
        }
    }
    // Scientific-boundary value pins: these discriminators are NOT free booleans.
    // An NCP frame is a control/simulation artifact, never a calibrated posterior
    // — so where they appear they MUST read calibrated_posterior=false and
    // is_simulation_output=true. A peer asserting otherwise is rejected, not
    // silently trusted. (Mirrors the proto "always false"/"always true" contract
    // and the ObservationFrame::default() invariant.)
    match kind {
        "observation_frame" => check_scientific_boundary(obj, kind)?,
        "session_opened" => {
            if let Some(p) = obj.get("provenance").and_then(|v| v.as_object()) {
                check_scientific_boundary(p, "session_opened.provenance")?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Enforce the scientific-boundary discriminators where present: an absent field
/// is fine (serde default supplies the safe value), but a present field carrying
/// the wrong value is a typed rejection — never coerced.
fn check_scientific_boundary(
    obj: &serde_json::Map<String, serde_json::Value>,
    ctx: &str,
) -> Result<(), ValidationError> {
    if let Some(v) = obj.get("calibrated_posterior") {
        if v.as_bool() != Some(false) {
            return Err(ValidationError(format!(
                "{ctx}: calibrated_posterior must be false (an NCP frame is sim \
                 output, never a calibrated posterior), got {v}"
            )));
        }
    }
    if let Some(v) = obj.get("is_simulation_output") {
        if v.as_bool() != Some(true) {
            return Err(ValidationError(format!(
                "{ctx}: is_simulation_output must be true (NCP frames are \
                 simulation output), got {v}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_version_rejects_malformed_minor_no_coercion() {
        // core-wire-1: a present-but-garbage minor or a trailing component must
        // REJECT (Err in strict mode), never silently coerce to minor 0. Tested
        // here against the live "0.4": none of these may parse to (0, 4).
        for bad in [
            "0.GARBAGE",
            "0.4.1",
            "0.4x",
            "0.",
            "0.4.0",
            "x.4",
            "0.0.0.0",
        ] {
            assert!(
                check_version(bad, true).is_err(),
                "malformed version {bad:?} must be rejected, not coerced"
            );
        }
        // Exact match passes; a missing minor means 0 and so mismatches 0.4.
        assert_eq!(check_version("0.4", true), Ok(true));
        assert!(check_version("0", true).is_err(), "0 -> (0,0) != (0,4)");
        // Non-strict mode surfaces the same rejection as Ok(false), not a coerced pass.
        assert_eq!(check_version("0.1", false), Ok(false));
    }

    #[test]
    fn contract_hash_matches_proto() {
        // Drift guard: recompute the canonical contract hash of the real proto and
        // assert it equals the pinned CONTRACT_HASH, so any *semantic* proto edit
        // must bump the constant (a comment-only edit must NOT — see below).
        let proto = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../proto/ncp.proto"))
            .expect("proto/ncp.proto readable from the workspace");
        assert_eq!(
            contract_hash_of_proto(&proto),
            CONTRACT_HASH,
            "proto's semantic contract changed without bumping CONTRACT_HASH (or vice versa)"
        );
    }

    #[test]
    fn contract_hash_ignores_comments_and_formatting() {
        let proto = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../proto/ncp.proto"))
            .expect("proto/ncp.proto readable from the workspace");
        let base = contract_hash_of_proto(&proto);

        // Inserting comments and blank lines must NOT change the contract hash.
        let mut commented = String::from_utf8_lossy(&proto).into_owned();
        commented
            .push_str("\n// a brand-new trailing comment\n/* and a block\n   comment */\n\n\n");
        commented = commented.replace(
            "message OpenSession {",
            "message OpenSession { // inline note",
        );
        assert_eq!(
            contract_hash_of_proto(commented.as_bytes()),
            base,
            "comment/whitespace-only edits must not change the contract hash"
        );

        // A NAMING-ONLY change (the proto `package`) must NOT change the hash — the
        // wire is identical, only the codegen namespace differs (the v0.4 decoupling
        // of `engram.ncp.v0 -> ncp.v0` is hash-neutral by construction).
        let renamed =
            String::from_utf8_lossy(&proto).replace("package ncp.v0;", "package engram.ncp.v0;");
        assert_eq!(
            contract_hash_of_proto(renamed.as_bytes()),
            base,
            "a package/naming-only change must NOT change the contract hash"
        );
        // A top-level option line must also be hash-neutral (non-wire metadata).
        let optioned = format!(
            "{}\noption go_package = \"x\";\n",
            String::from_utf8_lossy(&proto)
        );
        assert_eq!(
            contract_hash_of_proto(optioned.as_bytes()),
            base,
            "a top-level option must not change the contract hash"
        );

        // A real wire change (a new field) MUST change the hash.
        let semantic = String::from_utf8_lossy(&proto).replace(
            "string ncp_version = 1;",
            "string ncp_version = 1;\n  string injected = 99;",
        );
        assert_ne!(
            contract_hash_of_proto(semantic.as_bytes()),
            base,
            "a semantic wire change must change the contract hash"
        );

        // A `//` inside a string literal must be preserved (not treated as a comment).
        assert!(
            String::from_utf8(canonical_proto(b"string k = 1; // c\nstring s = \"a//b\";"))
                .unwrap()
                .contains("\"a//b\""),
            "string-literal contents must survive canonicalization"
        );
    }

    #[test]
    fn required_fields_match_the_schemas() {
        // Drift guard: required_fields() (the validator's source of truth) MUST
        // equal each JSON Schema's `required` array, so the Rust validator and the
        // schema corpus cannot silently disagree about what a wire message must
        // carry. Also asserts every schema `kind` has a required_fields() entry.
        use std::collections::BTreeSet;
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../schemas");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).expect("schemas/ readable from the workspace") {
            let path = entry.unwrap().path();
            if !path.to_string_lossy().ends_with(".schema.json") {
                continue;
            }
            let schema: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            let Some(kind) = schema["properties"]["kind"]["const"].as_str() else {
                continue;
            };
            let schema_required: BTreeSet<String> = schema
                .get("required")
                .and_then(|r| r.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let rf = required_fields(kind)
                .unwrap_or_else(|| panic!("schema kind {kind:?} has no required_fields() entry"));
            let rf_set: BTreeSet<String> = rf.iter().map(|s| (*s).to_string()).collect();
            assert_eq!(
                rf_set,
                schema_required,
                "required_fields({kind:?}) disagrees with {}'s `required`",
                path.file_name().unwrap().to_string_lossy()
            );
            checked += 1;
        }
        assert_eq!(
            checked, 13,
            "expected all 13 schema kinds, checked {checked}"
        );
    }

    #[test]
    fn negotiate_gates_version_advisory_contract() {
        // Version is the HARD gate; contract hash is ADVISORY.
        assert_eq!(
            negotiate(NCP_VERSION, Some(CONTRACT_HASH)),
            Ok(ContractStatus::Match)
        );
        assert_eq!(
            negotiate(NCP_VERSION, None),
            Ok(ContractStatus::NotAdvertised)
        );
        // Hash mismatch does NOT fail the handshake — it is surfaced as advisory so a
        // version-compatible flow (e.g. one peer added an optional field) keeps working.
        let status = negotiate(NCP_VERSION, Some("deadbeefdeadbeef")).expect("version ok");
        assert!(matches!(status, ContractStatus::Mismatch { .. }));
        assert!(!status.is_match());
        assert!(status.advisory().is_some());
        // Version mismatch still rejects (hard gate), regardless of the hash.
        assert!(negotiate("0.1", Some(CONTRACT_HASH)).is_err());
    }

    #[test]
    fn verify_contract_strict_still_fails_closed_opt_in() {
        // The opt-in strict path still rejects a mismatch, for safety-certified configs.
        assert!(verify_contract(Some(CONTRACT_HASH)).is_ok());
        assert!(verify_contract(None).is_ok());
        assert!(verify_contract(Some("deadbeefdeadbeef")).is_err());
    }

    #[test]
    fn diagnose_version_flags_mismatch() {
        // Compatible frame -> None; an incompatible version -> Some(err).
        let ok = format!(r#"{{"kind":"sensor_frame","ncp_version":"{NCP_VERSION}"}}"#);
        assert!(diagnose_version(ok.as_bytes()).is_none());
        assert!(diagnose_version(br#"{"kind":"sensor_frame","ncp_version":"0.1"}"#).is_some());
        // No version / unparseable -> None (best-effort, never panics).
        assert!(diagnose_version(br#"{"kind":"sensor_frame"}"#).is_none());
        assert!(diagnose_version(b"not json").is_none());
    }

    #[test]
    fn command_frame_absent_mode_deserializes_to_hold() {
        // Fail-safe: a wire CommandFrame that OMITS `mode` must NOT actuate. An
        // untrusted/partial frame (no mode field) deserializes to HOLD, not Active.
        let cmd: CommandFrame =
            serde_json::from_str(r#"{"kind":"command_frame","seq":1}"#).expect("parses");
        assert_eq!(
            cmd.mode,
            Mode::Hold,
            "a CommandFrame with no `mode` must default to HOLD on the wire"
        );
    }

    #[test]
    fn command_frame_unknown_mode_deserializes_to_hold() {
        // Fail-safe: an UNRECOGNIZED mode string must HOLD — not actuate, and not
        // error the whole frame. A peer sending a mode this build does not know
        // (e.g. a future "creep") degrades safely to HOLD.
        let cmd: CommandFrame =
            serde_json::from_str(r#"{"kind":"command_frame","seq":1,"mode":"creep"}"#)
                .expect("unknown mode must parse, not error the frame");
        assert_eq!(cmd.mode, Mode::Hold, "unknown mode -> HOLD (fail-safe)");
        // Known modes still map and serialize back to their lowercase wire string.
        let active: CommandFrame =
            serde_json::from_str(r#"{"kind":"command_frame","mode":"active"}"#).unwrap();
        assert_eq!(active.mode, Mode::Active);
        assert!(serde_json::to_string(&active)
            .unwrap()
            .contains("\"active\""));
    }
}
