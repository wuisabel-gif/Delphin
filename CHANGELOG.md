# Changelog

All notable changes to Delphin are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - Unreleased

### Added

- `delphin replay` for comparing recorded decisions with another arbiter policy.
- Live terminal-size propagation to the wrapped PTY.
- Immediate idle detection through configurable ready markers.
- A minimum busy-time guard for tools that work silently.
- A startup wordmark that makes the active wrapper visible.
- Conformance coverage for silent processes, process crashes, and abrupt link loss.

### Changed

- Agent output and queued prompts that remain during a crash are recorded in the
  local memory database.
- The README now documents replay, current installation paths, and recorded demos.

### Fixed

- Interrupt-driven process termination now shuts down cleanly.
- The Claude plugin manifest no longer contains an unsupported `skills` field.

## [0.2.0] - 2026-07-03

### Added

- Claude and Codex presets with live type-ahead defaults.
- Configurable interrupt words, ready markers, and minimum busy duration.
- End-to-end PTY, supervisor, arbiter, queue, and memory coverage.

## [0.1.0] - 2026-06-28

### Added

- Initial PTY wrapper, prompt queue, heuristic arbiter, and local SQLite memory.

[0.3.0]: https://github.com/wuisabel-gif/Delphin/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/wuisabel-gif/Delphin/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/wuisabel-gif/Delphin/releases/tag/v0.1.0
