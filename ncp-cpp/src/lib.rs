//! # ncp-cpp — C ABI for the NCP Rust core
//!
//! A stable `extern "C"` surface so **C and C++** projects use the canonical Rust
//! implementation of NCP (version guard, key scheme, rate codec, action-plane
//! safety governor, message validation) rather than reimplementing the wire — the
//! same guarantee `ncp-python` gives Python. The C header is `include/ncp.h`; a
//! worked example is `examples/demo.cpp`.
//!
//! String returns are heap-allocated UTF-8 C strings the caller must release with
//! [`ncp_string_free`]; `NULL` signals an error (malformed input). Inputs are
//! NUL-terminated UTF-8; JSON arguments/returns match the NCP wire exactly.

use ncp_core::{CodecSpec, CommandFrame, Keys, Map, Mode, SafetyGovernor, SafetyLimits, SensorFrame};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// Borrow a C string as `&str` (None if null/invalid UTF-8).
///
/// # Safety
/// `p` must be NULL or a valid NUL-terminated C string for the call's duration.
unsafe fn cstr_in<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        None
    } else {
        // SAFETY: caller guarantees `p` is a valid NUL-terminated C string.
        CStr::from_ptr(p).to_str().ok()
    }
}

/// Allocate a C string the caller frees with [`ncp_string_free`]; NULL on interior-NUL.
fn cstr_out(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a string returned by any `ncp_*` function.
///
/// # Safety
/// `s` must be NULL or a pointer previously returned by this library and not yet freed.
#[no_mangle]
pub unsafe extern "C" fn ncp_string_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: `s` was produced by `CString::into_raw` in this library.
        drop(CString::from_raw(s));
    }
}

/// The NCP protocol version (e.g. "0.1"). Caller frees.
#[no_mangle]
pub extern "C" fn ncp_version() -> *mut c_char {
    cstr_out(ncp_core::NCP_VERSION.to_string())
}

/// The default realm ("engram/ncp"). Caller frees.
#[no_mangle]
pub extern "C" fn ncp_default_realm() -> *mut c_char {
    cstr_out(ncp_core::DEFAULT_REALM.to_string())
}

/// 1 if `version` is major-compatible, 0 if not, -1 if unparseable/null.
///
/// # Safety
/// `version` must be NULL or a valid C string.
#[no_mangle]
pub unsafe extern "C" fn ncp_check_version(version: *const c_char, strict: bool) -> i32 {
    match cstr_in(version) {
        None => -1,
        Some(v) => match ncp_core::check_version(v, strict) {
            Ok(true) => 1,
            Ok(false) => 0,
            Err(_) => -1,
        },
    }
}

unsafe fn key_with(realm: *const c_char, f: impl FnOnce(&Keys) -> String) -> *mut c_char {
    let realm = cstr_in(realm).unwrap_or(ncp_core::DEFAULT_REALM);
    cstr_out(f(&Keys::new(realm.to_string())))
}

/// `{realm}/rpc`. Caller frees.
/// # Safety
/// `realm` must be NULL or a valid C string.
#[no_mangle]
pub unsafe extern "C" fn ncp_key_rpc(realm: *const c_char) -> *mut c_char {
    key_with(realm, |k| k.rpc())
}

/// `{realm}/session/{id}/sensor`. Caller frees.
/// # Safety
/// `realm`/`session_id` must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ncp_key_sensor(realm: *const c_char, session_id: *const c_char) -> *mut c_char {
    let sid = cstr_in(session_id).unwrap_or("").to_string();
    key_with(realm, |k| k.sensor(&sid))
}

/// `{realm}/session/{id}/command`. Caller frees.
/// # Safety
/// `realm`/`session_id` must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ncp_key_command(realm: *const c_char, session_id: *const c_char) -> *mut c_char {
    let sid = cstr_in(session_id).unwrap_or("").to_string();
    key_with(realm, |k| k.command(&sid))
}

/// `{realm}/session/{id}/observation`. Caller frees.
/// # Safety
/// `realm`/`session_id` must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ncp_key_observation(realm: *const c_char, session_id: *const c_char) -> *mut c_char {
    let sid = cstr_in(session_id).unwrap_or("").to_string();
    key_with(realm, |k| k.observation(&sid))
}

/// Rate-encode a `SensorFrame` JSON to `{population: rate_hz}` JSON. `sensor_json`
/// may be NULL/"null". Returns NULL on malformed input. Caller frees.
/// # Safety
/// Arguments must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ncp_encode_rates(codec_json: *const c_char, sensor_json: *const c_char) -> *mut c_char {
    let Some(codec_s) = cstr_in(codec_json) else { return std::ptr::null_mut() };
    let Ok(codec) = serde_json::from_str::<CodecSpec>(codec_s) else { return std::ptr::null_mut() };
    let sensor = match cstr_in(sensor_json) {
        Some(s) if !s.trim().is_empty() && s.trim() != "null" => {
            match serde_json::from_str::<SensorFrame>(s) {
                Ok(sf) => Some(sf),
                Err(_) => return std::ptr::null_mut(),
            }
        }
        _ => None,
    };
    match serde_json::to_string(&codec.encode(sensor.as_ref())) {
        Ok(s) => cstr_out(s),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Rate-decode `{population: rate_hz}` JSON to a `CommandFrame` JSON (mode=active,
/// frame_id="world"). Returns NULL on malformed input. Caller frees.
/// # Safety
/// Arguments must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ncp_decode_command(
    codec_json: *const c_char,
    rates_json: *const c_char,
    t: f64,
    seq: i64,
) -> *mut c_char {
    let (Some(codec_s), Some(rates_s)) = (cstr_in(codec_json), cstr_in(rates_json)) else {
        return std::ptr::null_mut();
    };
    let (Ok(codec), Ok(rates)) =
        (serde_json::from_str::<CodecSpec>(codec_s), serde_json::from_str::<Map<f64>>(rates_s))
    else {
        return std::ptr::null_mut();
    };
    let cmd = codec.decode(&rates, t, seq, "world", Mode::Active);
    match serde_json::to_string(&cmd) {
        Ok(s) => cstr_out(s),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Apply the action-plane safety governor to a `CommandFrame` JSON; returns the
/// governed JSON, or NULL on malformed input. `sensor_json` may be NULL.
/// `last_sensor_s < 0` means "no sensor yet" (forces HOLD). Caller frees.
/// # Safety
/// Arguments must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ncp_govern(
    limits_json: *const c_char,
    command_json: *const c_char,
    now_s: f64,
    sensor_json: *const c_char,
    last_sensor_s: f64,
) -> *mut c_char {
    let (Some(lim_s), Some(cmd_s)) = (cstr_in(limits_json), cstr_in(command_json)) else {
        return std::ptr::null_mut();
    };
    let (Ok(limits), Ok(command)) =
        (serde_json::from_str::<SafetyLimits>(lim_s), serde_json::from_str::<CommandFrame>(cmd_s))
    else {
        return std::ptr::null_mut();
    };
    let sensor = match cstr_in(sensor_json) {
        Some(s) if !s.trim().is_empty() && s.trim() != "null" => serde_json::from_str::<SensorFrame>(s).ok(),
        _ => None,
    };
    let last = if last_sensor_s < 0.0 { None } else { Some(last_sensor_s) };
    let out = SafetyGovernor::new(limits).govern(&command, sensor.as_ref(), now_s, last);
    match serde_json::to_string(&out) {
        Ok(s) => cstr_out(s),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Validate an NCP message JSON of a given `kind` (parse → re-serialize through the
/// Rust type). Returns canonical JSON, or NULL on malformed/unknown. Caller frees.
/// # Safety
/// Arguments must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ncp_validate(kind: *const c_char, json: *const c_char) -> *mut c_char {
    use ncp_core::*;
    let (Some(kind), Some(json)) = (cstr_in(kind), cstr_in(json)) else {
        return std::ptr::null_mut();
    };
    macro_rules! rt {
        ($t:ty) => {
            match serde_json::from_str::<$t>(json).and_then(|v| serde_json::to_string(&v)) {
                Ok(s) => cstr_out(s),
                Err(_) => std::ptr::null_mut(),
            }
        };
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
        _ => std::ptr::null_mut(),
    }
}
