//! # ncp-cpp — C ABI for the NCP Rust core
//!
//! A stable `extern "C"` surface so **C and C++** projects use the canonical Rust
//! implementation of NCP (version guard, key scheme, rate codec, action-plane
//! safety governor, message validation) rather than reimplementing the wire — the
//! same guarantee `ncp-python` gives Python. The C header is `include/ncp.h`; a
//! worked example is `examples/demo.cpp`.
//!
//! String returns are heap-allocated UTF-8 C strings the caller must release with
//! [`ncp_string_free`]; `NULL` signals malformed input or an internal error.
//! Inputs are NUL-terminated UTF-8; JSON arguments/returns match the NCP wire
//! exactly.
//!
//! ## Unwind safety
//! Every `extern "C"` body is wrapped in [`std::panic::catch_unwind`] and returns
//! its NULL/-1 sentinel if a Rust panic is caught — a panic must never unwind
//! across the C ABI (that is undefined behaviour). This is independent of the
//! final binary's `panic` strategy: a staticlib cannot assume `panic=abort`.

use ncp_core::{
    CodecSpec, CommandFrame, Keys, Map, Mode, SafetyGovernor, SafetyLimits, SensorFrame,
};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Run an FFI body, returning `sentinel` if it panics (so no unwind crosses the
/// C ABI). `AssertUnwindSafe` is sound here: on panic we discard all locals and
/// return a fresh sentinel, observing no broken invariants.
fn ffi_guard<T>(sentinel: T, body: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(sentinel)
}

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
    ffi_guard((), || {
        if !s.is_null() {
            // SAFETY: `s` was produced by `CString::into_raw` in this library.
            drop(CString::from_raw(s));
        }
    })
}

/// The NCP protocol version (e.g. "0.1"). Caller frees.
#[no_mangle]
pub extern "C" fn ncp_version() -> *mut c_char {
    ffi_guard(std::ptr::null_mut(), || {
        cstr_out(ncp_core::NCP_VERSION.to_string())
    })
}

/// The default realm ("engram/ncp"). Caller frees.
#[no_mangle]
pub extern "C" fn ncp_default_realm() -> *mut c_char {
    ffi_guard(std::ptr::null_mut(), || {
        cstr_out(ncp_core::DEFAULT_REALM.to_string())
    })
}

/// 1 if `version` is major-compatible, 0 if not, -1 if unparseable/null.
///
/// # Safety
/// `version` must be NULL or a valid C string.
#[no_mangle]
pub unsafe extern "C" fn ncp_check_version(version: *const c_char, strict: bool) -> i32 {
    ffi_guard(-1, || match cstr_in(version) {
        None => -1,
        Some(v) => match ncp_core::check_version(v, strict) {
            Ok(true) => 1,
            Ok(false) => 0,
            Err(_) => -1,
        },
    })
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
    ffi_guard(std::ptr::null_mut(), || key_with(realm, |k| k.rpc()))
}

/// `{realm}/session/{id}/sensor`. Caller frees.
/// # Safety
/// `realm`/`session_id` must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ncp_key_sensor(
    realm: *const c_char,
    session_id: *const c_char,
) -> *mut c_char {
    ffi_guard(std::ptr::null_mut(), || {
        let sid = cstr_in(session_id).unwrap_or("").to_string();
        key_with(realm, |k| k.sensor(&sid))
    })
}

/// `{realm}/session/{id}/command`. Caller frees.
/// # Safety
/// `realm`/`session_id` must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ncp_key_command(
    realm: *const c_char,
    session_id: *const c_char,
) -> *mut c_char {
    ffi_guard(std::ptr::null_mut(), || {
        let sid = cstr_in(session_id).unwrap_or("").to_string();
        key_with(realm, |k| k.command(&sid))
    })
}

/// `{realm}/session/{id}/observation`. Caller frees.
/// # Safety
/// `realm`/`session_id` must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ncp_key_observation(
    realm: *const c_char,
    session_id: *const c_char,
) -> *mut c_char {
    ffi_guard(std::ptr::null_mut(), || {
        let sid = cstr_in(session_id).unwrap_or("").to_string();
        key_with(realm, |k| k.observation(&sid))
    })
}

/// Rate-encode a `SensorFrame` JSON to `{population: rate_hz}` JSON. `sensor_json`
/// may be NULL/"null". Returns NULL on malformed input. Caller frees.
/// # Safety
/// Arguments must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ncp_encode_rates(
    codec_json: *const c_char,
    sensor_json: *const c_char,
) -> *mut c_char {
    ffi_guard(std::ptr::null_mut(), || {
        let Some(codec_s) = cstr_in(codec_json) else {
            return std::ptr::null_mut();
        };
        let Ok(codec) = serde_json::from_str::<CodecSpec>(codec_s) else {
            return std::ptr::null_mut();
        };
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
    })
}

/// Parse an NCP mode string to [`Mode`]; `None` on an unknown mode.
fn parse_mode(s: &str) -> Option<Mode> {
    match s {
        "init" => Some(Mode::Init),
        "active" => Some(Mode::Active),
        "hold" => Some(Mode::Hold),
        "estop" => Some(Mode::Estop),
        _ => None,
    }
}

/// Rate-decode `{population: rate_hz}` JSON to a `CommandFrame` JSON. `frame_id`
/// may be NULL (=> "world"); `mode` is one of init/active/hold/estop and may be
/// NULL (=> "active") — an unknown mode returns NULL. Returns NULL on malformed
/// input or internal error. Caller frees.
/// # Safety
/// Arguments must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ncp_decode_command(
    codec_json: *const c_char,
    rates_json: *const c_char,
    t: f64,
    seq: i64,
    frame_id: *const c_char,
    mode: *const c_char,
) -> *mut c_char {
    ffi_guard(std::ptr::null_mut(), || {
        let (Some(codec_s), Some(rates_s)) = (cstr_in(codec_json), cstr_in(rates_json)) else {
            return std::ptr::null_mut();
        };
        let (Ok(codec), Ok(rates)) = (
            serde_json::from_str::<CodecSpec>(codec_s),
            serde_json::from_str::<Map<f64>>(rates_s),
        ) else {
            return std::ptr::null_mut();
        };
        let frame_id = cstr_in(frame_id).unwrap_or("world");
        let Some(mode) = parse_mode(cstr_in(mode).unwrap_or("active")) else {
            return std::ptr::null_mut();
        };
        let cmd = codec.decode(&rates, t, seq, frame_id, mode);
        match serde_json::to_string(&cmd) {
            Ok(s) => cstr_out(s),
            Err(_) => std::ptr::null_mut(),
        }
    })
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
    ffi_guard(std::ptr::null_mut(), || {
        let (Some(lim_s), Some(cmd_s)) = (cstr_in(limits_json), cstr_in(command_json)) else {
            return std::ptr::null_mut();
        };
        let (Ok(limits), Ok(command)) = (
            serde_json::from_str::<SafetyLimits>(lim_s),
            serde_json::from_str::<CommandFrame>(cmd_s),
        ) else {
            return std::ptr::null_mut();
        };
        let sensor = match cstr_in(sensor_json) {
            Some(s) if !s.trim().is_empty() && s.trim() != "null" => {
                serde_json::from_str::<SensorFrame>(s).ok()
            }
            _ => None,
        };
        let last = if last_sensor_s < 0.0 {
            None
        } else {
            Some(last_sensor_s)
        };
        // `govern` latches ESTOP and so takes `&mut self`; this FFI wrapper is
        // one-shot (fresh governor per call) so the latch never persists.
        let mut gov = SafetyGovernor::new(limits);
        let out = gov.govern(&command, sensor.as_ref(), now_s, last);
        match serde_json::to_string(&out) {
            Ok(s) => cstr_out(s),
            Err(_) => std::ptr::null_mut(),
        }
    })
}

/// Validate an NCP message JSON of a given `kind` (parse → re-serialize through the
/// Rust type). Returns canonical JSON, or NULL on malformed/unknown. Caller frees.
/// # Safety
/// Arguments must be NULL or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ncp_validate(kind: *const c_char, json: *const c_char) -> *mut c_char {
    use ncp_core::*;
    ffi_guard(std::ptr::null_mut(), || {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Take ownership of a returned C string and free it via the FFI path.
    unsafe fn take(p: *mut c_char) -> Option<String> {
        if p.is_null() {
            None
        } else {
            let s = CStr::from_ptr(p).to_str().unwrap().to_string();
            ncp_string_free(p);
            Some(s)
        }
    }

    fn cstr(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    #[test]
    fn ffi_guard_catches_panic_and_returns_sentinel() {
        // A panicking body must NOT unwind across the (simulated) C ABI; it returns
        // the sentinel instead (FIX 8). Silence the default panic print for noise.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let out: *mut c_char = ffi_guard(std::ptr::null_mut(), || panic!("boom"));
        let code: i32 = ffi_guard(-1, || panic!("boom"));
        std::panic::set_hook(prev);
        assert!(out.is_null());
        assert_eq!(code, -1);
    }

    #[test]
    fn decode_command_emits_hold_and_estop_and_frame_id() {
        let codec = cstr("{}");
        let rates = cstr("{}");
        // SHOULD-FIX C decode completeness: the C path can now emit non-active modes
        // and a non-"world" frame id.
        for (mode, expect) in [
            ("hold", "\"mode\":\"hold\""),
            ("estop", "\"mode\":\"estop\""),
        ] {
            let m = cstr(mode);
            let fid = cstr("base_link");
            let out = unsafe {
                take(ncp_decode_command(
                    codec.as_ptr(),
                    rates.as_ptr(),
                    0.0,
                    1,
                    fid.as_ptr(),
                    m.as_ptr(),
                ))
            }
            .expect("valid mode must decode");
            assert!(out.contains(expect), "mode {mode}: {out}");
            assert!(out.contains("base_link"), "frame_id missing: {out}");
        }
    }

    #[test]
    fn decode_command_defaults_are_world_and_active() {
        let codec = cstr("{}");
        let rates = cstr("{}");
        // NULL frame_id => "world", NULL mode => "active".
        let out = unsafe {
            take(ncp_decode_command(
                codec.as_ptr(),
                rates.as_ptr(),
                0.0,
                0,
                std::ptr::null(),
                std::ptr::null(),
            ))
        }
        .expect("defaults must decode");
        assert!(out.contains("\"mode\":\"active\""), "{out}");
        assert!(out.contains("world"), "{out}");
    }

    #[test]
    fn decode_command_rejects_unknown_mode() {
        let codec = cstr("{}");
        let rates = cstr("{}");
        let bad = cstr("turbo");
        let out = unsafe {
            ncp_decode_command(
                codec.as_ptr(),
                rates.as_ptr(),
                0.0,
                0,
                std::ptr::null(),
                bad.as_ptr(),
            )
        };
        assert!(out.is_null(), "unknown mode must return NULL");
    }

    #[test]
    fn null_and_garbage_inputs_return_null_not_crash() {
        unsafe {
            assert!(ncp_encode_rates(std::ptr::null(), std::ptr::null()).is_null());
            assert!(ncp_validate(cstr("nope").as_ptr(), cstr("{}").as_ptr()).is_null());
            assert_eq!(ncp_check_version(std::ptr::null(), false), -1);
        }
    }
}
