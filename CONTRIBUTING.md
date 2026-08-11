# Contributing to Delphin

Thanks for your interest! Delphin is a small, local-first Rust CLI — a duplex
wrapper for AI agent CLIs. Contributions of all sizes are welcome.

By participating, you agree to follow the
[`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

## Development

For a guided first setup and test loop, see
[`docs/contributor-quickstart.md`](docs/contributor-quickstart.md).

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

## Releasing

Release preparation and publication are separate steps:

1. Move the pending changelog section from `Unreleased` to the release date.
2. Confirm the `Cargo.toml` version matches the changelog.
3. Run the full checks above and `cargo package`.
4. Merge the release preparation PR.
5. Create and push the matching signed tag.
6. Publish the crate with `cargo publish`.
7. Update and verify the Homebrew formula against the published archive.
8. Create the GitHub release from the changelog entry.

Do not advertise a version through the default install commands until both the
crate and Homebrew artifacts are available.

## Project layout

| File | Role |
|---|---|
| `src/lib.rs` | reusable public module surface |
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
