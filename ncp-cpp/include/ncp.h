/*
 * ncp.h — C / C++ ABI for the Neuro-Control Protocol (NCP) Rust core.
 *
 * So C and C++ projects use the canonical Rust implementation (version guard,
 * key scheme, rate codec, action-plane safety governor, message validation)
 * rather than reimplementing the wire — the same guarantee the Python (PyO3) and
 * TypeScript (ts-rs) bindings give. Link against `ncp_cpp` (staticlib or cdylib
 * built by `cargo build -p ncp-cpp`).
 *
 * Memory: every `char*` return is a heap-allocated UTF-8 C string the caller
 * MUST release with `ncp_string_free`. A NULL return signals malformed input.
 * String arguments are NUL-terminated UTF-8; JSON args/returns match the NCP
 * wire exactly (see ncp.proto / schemas).
 */
#ifndef NCP_H
#define NCP_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Release any string returned by an ncp_* function. NULL is ignored. */
void ncp_string_free(char *s);

/* Protocol version (e.g. "0.1"). Caller frees. */
char *ncp_version(void);

/* Default realm ("engram/ncp"). Caller frees. */
char *ncp_default_realm(void);

/* 1 if major-compatible, 0 if not, -1 if unparseable/NULL. */
int32_t ncp_check_version(const char *version, bool strict);

/* Key-expression builders. Caller frees. */
char *ncp_key_rpc(const char *realm);
char *ncp_key_sensor(const char *realm, const char *session_id);
char *ncp_key_command(const char *realm, const char *session_id);
char *ncp_key_observation(const char *realm, const char *session_id);

/* Rate codec. JSON in / JSON out. NULL on malformed input. Caller frees. */
char *ncp_encode_rates(const char *codec_json, const char *sensor_json);
char *ncp_decode_command(const char *codec_json, const char *rates_json,
                         double t, int64_t seq);

/* Action-plane safety governor. last_sensor_s < 0 => "no sensor yet" (HOLD).
 * NULL on malformed input. Caller frees. */
char *ncp_govern(const char *limits_json, const char *command_json, double now_s,
                 const char *sensor_json, double last_sensor_s);

/* Validate an NCP message of `kind` (parse->reserialize). NULL on malformed/
 * unknown kind. Caller frees. */
char *ncp_validate(const char *kind, const char *json);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* NCP_H */
