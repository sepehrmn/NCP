# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- Renamed the protocol to the **Neuro-Cybernetic Protocol (NCP)**.
- Vendored the spec, `.proto` definitions, and JSON schemas into the SDK so the
  wire contract ships with the reference implementation rather than living out of
  tree.

### Added
- Wire conformance test that pins the on-the-wire contract against the vendored
  spec/proto/schemas.
- Release scaffolding (LICENSE, CITATION.cff, SECURITY.md, this changelog,
  crates.io metadata) and CI.

## [0.1.0] - 2026-06-16

- Initial release of the protocol and Rust reference SDK (`ncp-core`,
  `ncp-zenoh`, `ncp-gateway`, `ncp-python`, `ncp-cpp`).

[Unreleased]: https://github.com/sepehrmn/ncp/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/sepehrmn/ncp/releases/tag/v0.1.0
