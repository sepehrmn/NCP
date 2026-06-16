//! # ncp (Python) — PyO3 bindings for the NCP Rust core
//!
//! So Python projects use the **canonical Rust implementation** of NCP rather
//! than reimplementing the wire: the version guard, the key scheme, the rate
//! codec, the action-plane safety governor, and message validation all come from
//! `ncp-core`. Engram's NEST server keeps its Pydantic models for the server side,
//! but any Python peer can compute keys, encode/decode, and validate frames
//! through this module and be guaranteed wire-identical to the Rust and TS peers.
//!
//! Build the importable extension with maturin (`maturin develop -m
//! ncp-python/Cargo.toml --features extension-module`). The `extension-module`
//! feature is **off by default** so `cargo build`/`check`/`test --workspace`
//! works on Linux/Windows (enabling it unconditionally suppresses the libpython
//! link and breaks the non-Python build/test); maturin must enable it explicitly.
//!
//! ```python
//! import ncp
//! ncp.NCP_VERSION                      # "0.1"
//! k = ncp.Keys("engram/ncp")
//! k.command("uav3")                    # "engram/ncp/session/uav3/command"
//! ncp.decode_command(codec_json, '{"vel_x":200.0}', t=0.0, seq=7)  # CommandFrame JSON
//! ```

use ncp_core::{
    ChannelValue, CodecSpec, CommandFrame, Keys as CoreKeys, Map, Mode, SafetyGovernor,
    SafetyLimits, SensorFrame,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

fn val<E: std::fmt::Display>(e: E) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// The NCP key scheme (the three planes + control RPC), so Python addresses the
/// same keys as the Rust/TS peers.
#[pyclass]
struct Keys {
    inner: CoreKeys,
}

#[pymethods]
impl Keys {
    #[new]
    #[pyo3(signature = (realm = None))]
    fn new(realm: Option<String>) -> Self {
        Keys {
            inner: CoreKeys::new(realm.unwrap_or_else(|| ncp_core::DEFAULT_REALM.to_string())),
        }
    }
    fn rpc(&self) -> String {
        self.inner.rpc()
    }
    fn sensor(&self, session_id: &str) -> String {
        self.inner.sensor(session_id)
    }
    fn sensor_named(&self, session_id: &str, name: &str) -> String {
        self.inner.sensor_named(session_id, name)
    }
    fn command(&self, session_id: &str) -> String {
        self.inner.command(session_id)
    }
    fn command_named(&self, session_id: &str, name: &str) -> String {
        self.inner.command_named(session_id, name)
    }
    fn observation(&self, session_id: &str) -> String {
        self.inner.observation(session_id)
    }
    fn session_glob(&self, session_id: &str) -> String {
        self.inner.session_glob(session_id)
    }
}

/// `True` if `version` is major-compatible with this NCP. Raises on a major
/// mismatch when `strict`.
#[pyfunction]
#[pyo3(signature = (version, strict = false))]
fn check_version(version: &str, strict: bool) -> PyResult<bool> {
    ncp_core::check_version(version, strict).map_err(val)
}

/// Rate-encode a `SensorFrame` JSON to `{population: rate_hz}` JSON, via the Rust
/// codec. `sensor_json` may be `"null"` for the no-sensor case.
#[pyfunction]
fn encode_rates(codec_json: &str, sensor_json: &str) -> PyResult<String> {
    let codec: CodecSpec = serde_json::from_str(codec_json).map_err(val)?;
    let sensor: Option<SensorFrame> =
        if sensor_json.trim().is_empty() || sensor_json.trim() == "null" {
            None
        } else {
            Some(serde_json::from_str(sensor_json).map_err(val)?)
        };
    let rates = codec.encode(sensor.as_ref());
    serde_json::to_string(&rates).map_err(val)
}

/// Rate-decode `{population: rate_hz}` JSON to a `CommandFrame` JSON, via the Rust
/// codec.
#[pyfunction]
#[pyo3(signature = (codec_json, rates_json, t = 0.0, seq = 0, frame_id = "world", mode = "active"))]
fn decode_command(
    codec_json: &str,
    rates_json: &str,
    t: f64,
    seq: i64,
    frame_id: &str,
    mode: &str,
) -> PyResult<String> {
    let codec: CodecSpec = serde_json::from_str(codec_json).map_err(val)?;
    let rates: Map<f64> = serde_json::from_str(rates_json).map_err(val)?;
    let mode = parse_mode(mode)?;
    let cmd = codec.decode(&rates, t, seq, frame_id, mode);
    serde_json::to_string(&cmd).map_err(val)
}

fn parse_mode(s: &str) -> PyResult<Mode> {
    Ok(match s {
        "init" => Mode::Init,
        "active" => Mode::Active,
        "hold" => Mode::Hold,
        "estop" => Mode::Estop,
        other => return Err(PyValueError::new_err(format!("unknown mode {other:?}"))),
    })
}

/// Apply the action-plane safety governor to a `CommandFrame` JSON, returning the
/// governed `CommandFrame` JSON (HOLD on a stale sensor, ESTOP on geofence breach,
/// speed clamp). `sensor_json`/`last_sensor_s` may be `None`.
#[pyfunction]
#[pyo3(signature = (limits_json, command_json, now_s, sensor_json = None, last_sensor_s = None))]
fn govern(
    limits_json: &str,
    command_json: &str,
    now_s: f64,
    sensor_json: Option<&str>,
    last_sensor_s: Option<f64>,
) -> PyResult<String> {
    let limits: SafetyLimits = serde_json::from_str(limits_json).map_err(val)?;
    let command: CommandFrame = serde_json::from_str(command_json).map_err(val)?;
    let sensor: Option<SensorFrame> = match sensor_json {
        Some(s) if !s.trim().is_empty() && s.trim() != "null" => {
            Some(serde_json::from_str(s).map_err(val)?)
        }
        _ => None,
    };
    // `govern` latches ESTOP, so it takes `&mut self`. This wrapper is one-shot
    // (fresh governor per call), so the latch never persists across FFI calls.
    let mut gov = SafetyGovernor::new(limits);
    let out = gov.govern(&command, sensor.as_ref(), now_s, last_sensor_s);
    serde_json::to_string(&out).map_err(val)
}

/// Validate an NCP message JSON of a given `kind` by parsing it through the Rust
/// type and re-serializing — raises `ValueError` on a message the Rust type
/// rejects, else returns its canonical JSON. This checks structural/serde
/// conformance to the wire schema (field names, types, required fields); it is
/// not a semantic/range check, so a structurally valid frame may still be
/// rejected downstream (e.g. by the safety governor).
#[pyfunction]
fn validate(kind: &str, json: &str) -> PyResult<String> {
    use ncp_core::*;
    macro_rules! rt {
        ($t:ty) => {{
            let v: $t = serde_json::from_str(json).map_err(val)?;
            serde_json::to_string(&v).map_err(val)
        }};
    }
    match kind {
        "open_session" => rt!(OpenSession),
        "session_opened" => rt!(SessionOpened),
        "step_request" => rt!(StepRequest),
        "run_request" => rt!(RunRequest),
        "stimulus_frame" => rt!(StimulusFrame),
        "observation_frame" => rt!(ObservationFrame),
        "close_session" => rt!(CloseSession),
        "session_closed" => rt!(SessionClosed),
        "sensor_frame" => rt!(SensorFrame),
        "command_frame" => rt!(CommandFrame),
        "control_status" => rt!(ControlStatus),
        "capabilities" => rt!(Capabilities),
        other => Err(PyValueError::new_err(format!(
            "unknown NCP message kind {other:?}"
        ))),
    }
}

/// Convenience: build a `ChannelValue` JSON (`{"data": [...], "unit": ...}`).
#[pyfunction]
#[pyo3(signature = (data, unit = None))]
fn channel_value(data: Vec<f64>, unit: Option<String>) -> PyResult<String> {
    serde_json::to_string(&ChannelValue { data, unit }).map_err(val)
}

#[pymodule]
fn ncp(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("NCP_VERSION", ncp_core::NCP_VERSION)?;
    m.add("DEFAULT_REALM", ncp_core::DEFAULT_REALM)?;
    m.add_class::<Keys>()?;
    m.add_function(wrap_pyfunction!(check_version, m)?)?;
    m.add_function(wrap_pyfunction!(encode_rates, m)?)?;
    m.add_function(wrap_pyfunction!(decode_command, m)?)?;
    m.add_function(wrap_pyfunction!(govern, m)?)?;
    m.add_function(wrap_pyfunction!(validate, m)?)?;
    m.add_function(wrap_pyfunction!(channel_value, m)?)?;
    Ok(())
}
