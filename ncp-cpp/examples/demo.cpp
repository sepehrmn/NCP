// Minimal C++ NCP client using the Rust core via the C ABI (ncp.h).
//
// Build (from ncp/):
//   cargo build -p ncp-cpp
//   c++ -std=c++17 -I ncp-cpp/include ncp-cpp/examples/demo.cpp \
//       -L target/debug -lncp_cpp -Wl,-rpath,target/debug -o /tmp/ncp_demo
//   /tmp/ncp_demo
//
// A C++ project would wrap these in RAII (a std::unique_ptr with ncp_string_free
// as the deleter) and a JSON library (nlohmann/json) — the point here is that the
// behavior comes from the one canonical Rust core, wire-identical to every peer.

#include "ncp.h"
#include <iostream>
#include <memory>
#include <string>

// RAII for the heap C strings the library returns.
struct NcpFree {
  void operator()(char *p) const { ncp_string_free(p); }
};
using NcpStr = std::unique_ptr<char, NcpFree>;

static std::string take(char *p) {
  NcpStr s(p);
  return s ? std::string(s.get()) : std::string("<null>");
}

int main() {
  std::cout << "NCP_VERSION   = " << take(ncp_version()) << "\n";
  std::cout << "DEFAULT_REALM = " << take(ncp_default_realm()) << "\n";
  std::cout << "command key   = "
            << take(ncp_key_command("engram/ncp", "uav3")) << "\n";
  std::cout << "check 0.1     = " << ncp_check_version("0.1", false) << "\n";
  std::cout << "check 1.0     = " << ncp_check_version("1.0", false) << "\n";

  const char *codec =
      "{\"encoder\":[],\"decoder\":[{\"population\":\"vel_x\",\"readout\":\"rate\","
      "\"command_channel\":\"velocity_setpoint\",\"component\":0,\"unit\":\"m/s\","
      "\"rate_range_hz\":[0,200],\"value_range\":[-1.5,1.5]}]}";
  std::cout << "decode(200hz) = "
            << take(ncp_decode_command(codec, "{\"vel_x\":200.0}", 0.0, 7,
                                       /*frame_id=*/nullptr, /*mode=*/nullptr))
            << "\n";

  std::string ok = take(ncp_validate(
      "open_session",
      "{\"session_id\":\"s1\",\"network\":{\"kind\":\"builtin\",\"ref\":\"iaf_psc_alpha\"}}"));
  bool valid = ok.find("\"kind\":\"open_session\"") != std::string::npos;
  std::cout << "validate ok   = " << (valid ? "true" : "false") << "\n";

  // Exit nonzero if anything basic is wrong, so the smoke test can assert.
  bool pass = take(ncp_version()) == "0.1" && ncp_check_version("0.1", false) == 1 &&
              ncp_check_version("1.0", false) == 0 && valid;
  std::cout << (pass ? "C++ NCP demo: OK" : "C++ NCP demo: FAILED") << "\n";
  return pass ? 0 : 1;
}
