# Contributing to Delphin

Thanks for your interest! Delphin is a small, local-first Rust CLI — a duplex
wrapper for AI agent CLIs. Contributions of all sizes are welcome.

## Development

```bash
cargo build
cargo test                      # unit tests (fast, deterministic)
cargo run -- --interrupt ctrl-c -- bash examples/mock-agent.sh   # try it
```

Run the end-to-end test (drives the binary against the mock agent; timing-based,
so it's `#[ignore]`d by default):

```bash
cargo test --test e2e -- --ignored
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
| `src/main.rs` | CLI parsing, wiring |
| `src/supervisor.rs` | PTY spawn, idle detection, event loop |
| `src/arbiter.rs` | `Arbiter` trait + default heuristic policy |
| `src/queue.rs` | prompt FIFO |
| `src/memory.rs` | self-contained SQLite log + ANSI stripping |
| `examples/mock-agent.sh` | fake "thinking" agent for testing |
| `tests/e2e.rs` | end-to-end integration test |

## Good first contributions

- **New arbiter policies** behind the `Arbiter` trait (e.g. an LLM-judge, a
  priority queue, or "questions interrupt / commands queue").
- **Better idle detection** — parse an agent's own "esc to interrupt" / ready
  markers instead of relying purely on output silence.
- **Per-agent presets** for `--interrupt` and `--idle-ms`.

## Conventions

- Rust 2021, formatted with `rustfmt`, lint-clean under `clippy`.
- Keep the default path **local-first and zero-setup**: new capabilities that
  need a network service or model should be optional, with a graceful fallback.

## License

By contributing, you agree your contributions are licensed under the project's
[MIT License](LICENSE).
