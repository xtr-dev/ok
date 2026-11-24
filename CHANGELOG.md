# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Repository setup: Cargo workspace (`okrun` package, `ok` binary), CC0
  licence, cargo-deny licence policy, cargo-about third-party notices, CI for
  format, clippy and tests on Linux, macOS and Windows, and a release workflow
  for six targets.
- `SPEC.md`: the full language specification draft for v0.1.0.
- `DECISIONS.md`: log of design decisions the brief left open.
- `examples/`: real-world scripts and Makefiles rewritten in ok, each next to
  the original.
- `ok.ok`: the repository's own build and admin tasks (runnable from M5; use
  the documented `cargo` commands until then).
- Placeholder `ok` binary that answers `ok self version`.
- `quiet` command option to hide a command's output unless it fails.
