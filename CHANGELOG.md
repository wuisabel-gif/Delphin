# Changelog

All notable changes to Delphin are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `:fix` sends MemoryWhale's latest suggested fix to the agent as a prompt to
  check, routed like any other prompt (queued if the agent is busy). Hints that
  include a fix now end with `· type :fix to send it`. The fix is never run, and
  control characters are stripped from hint output. Not available with
  `--passthrough`.

### Changed

- Library API: `HintWatcher::spawn` takes an `offer_fix` flag.

## [0.4.0] - 2026-09-30

### Added

- `--memorywhale` (or `memorywhale = true` in config): record turns through
  MemoryWhale's `mw turns`, so MemoryWhale redacts secrets and owns the schema,
  and show live "seen this before" hints for errors in the agent's output via
  `mw hint`. Needs MemoryWhale 0.15 or newer.
- `delphin --version`.

### Changed

- Library API: `supervisor::Settings` has a new `hints` field, and
  `MemoryLog::db_path` is replaced by `MemoryLog::location`.

## [0.3.0] - 2026-09-30

### Added

- `delphin replay` for comparing recorded decisions with another arbiter policy.
- Raw terminal passthrough, so full-screen agent UIs render correctly. (#10)
- A reusable library API for embedding Delphin's supervisor and arbiter. (#14)
- Live terminal-size propagation to the wrapped PTY.
- Immediate idle detection through configurable ready markers.
- A minimum busy-time guard for tools that work silently.
- A startup wordmark that makes the active wrapper visible.
- Conformance coverage for silent processes, process crashes, and abrupt link loss.

### Changed

- Agent output and queued prompts that remain during a crash are recorded in the
  local memory database.
- The README now documents replay, current installation paths, and recorded demos.
- CI runs on Linux and macOS, split by task, with a dependency audit. (#13, #32, #36)

### Fixed

- The wrapped process's exit status is passed through. (#5)
- Help arguments meant for the wrapped command are preserved. (#6)
- Stale ready markers no longer end a busy period early. (#7)
- Buffered process output is bounded. (#8)
- Local memory privacy is hardened. (#11)
- Timing options are validated. (#12)
- A shared memory database's schema is validated before use, for example when
  pointing `--db` at MemoryWhale. (#16)
- Non-Unicode environment values are preserved. (#18, thanks @alloutflo)
- Recall queries treat `%`, `_`, and `\` as literal characters instead of
  SQLite `LIKE` wildcards. (#42, thanks @floze-the-genius)
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

[Unreleased]: https://github.com/wuisabel-gif/Delphin/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/wuisabel-gif/Delphin/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/wuisabel-gif/Delphin/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/wuisabel-gif/Delphin/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/wuisabel-gif/Delphin/releases/tag/v0.1.0
