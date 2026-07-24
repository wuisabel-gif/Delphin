# Contributing to Delphin

Thanks for your interest! Delphin is a small, local-first Rust CLI — a duplex
wrapper for AI agent CLIs. Contributions of all sizes are welcome.

## Development

```bash
cargo build
cargo test --all                # unit + PTY conformance tests
cargo run -- --interrupt ctrl-c -- bash examples/mock-agent.sh   # try it
```

The PTY conformance tests run by default and take longer than the unit tests.
During focused development, run one integration target directly:

```bash
cargo test --test e2e
cargo test --test silent_agent_conformance
```

## Before you open a PR

CI runs these three checks — please run them locally first:

```bash
cargo fmt --all --check         # formatting
cargo clippy --all-targets -- -D warnings   # lints (treated as errors)
cargo test --all                # tests
```

`cargo fmt` (without `--check`) auto-formats.

## Project layout

| File | Role |
|---|---|
| `src/main.rs` | CLI parsing, wiring |
| `src/supervisor.rs` | PTY spawn, idle detection, event loop |
| `src/arbiter.rs` | `Arbiter` trait + default heuristic policy |
| `src/queue.rs` | prompt FIFO |
| `src/memory.rs` | self-contained SQLite log + ANSI stripping |
| `src/config.rs` | TOML configuration defaults and loading |
| `src/replay.rs` | historical arbiter-policy comparison |
| `examples/mock-agent.sh` | fake "thinking" agent for testing |
| `examples/*-agent.sh` | deterministic process-behavior fixtures |
| `tests/e2e.rs` | queue, release, interrupt, and shutdown integration |
| `tests/*_conformance.rs` | silent, crash, and link-loss scenarios |

## Good first contributions

- Add a golden transcript fixture for another command-line tool.
- Add CLI parsing and configuration error-message tests.
- Document a tested setup for another terminal or operating system.
- Add a focused arbiter policy with deterministic unit tests.

## Conventions

- Rust 2021, formatted with `rustfmt`, lint-clean under `clippy`.
- Keep the default path **local-first and zero-setup**: new capabilities that
  need a network service or model should be optional, with a graceful fallback.

## License

By contributing, you agree your contributions are licensed under the project's
[MIT License](LICENSE).
