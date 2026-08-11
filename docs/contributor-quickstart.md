# Contributor Quickstart

This guide gets a new contributor from a fresh checkout to a useful local test
loop. Delphin is a small Rust CLI, but many behaviors depend on a real PTY, so
the integration tests matter.

## Prerequisites

- Rust stable with `cargo`, `rustfmt`, and `clippy`.
- A Unix-like shell for the PTY fixtures. The conformance suite is tested on
  Linux and macOS.
- No model account is required for local development; use the mock agents in
  `examples/`.

## Setup

```bash
git clone https://github.com/wuisabel-gif/Delphin
cd Delphin
cargo build
```

Run the mock agent through Delphin:

```bash
cargo run -- --interrupt ctrl-c --ready 'you> ' -- bash examples/mock-agent.sh
```

Try typing one ordinary follow-up while the mock is busy, then an urgent prompt
such as `stop wrong thing`.

## Test Loop

Run the same checks CI runs:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all
```

For a narrower loop:

```bash
cargo test --lib
cargo test --test e2e
cargo test --test silent_agent_conformance
```

Use `cargo fmt` without `--check` to apply formatting.

## Where To Start

- CLI flags and presets: `src/main.rs`
- Prompt-routing policy: `src/arbiter.rs`
- PTY event loop and idle detection: `src/supervisor.rs`
- FIFO prompt storage: `src/queue.rs`
- SQLite memory log: `src/memory.rs`
- Policy replay over recorded sessions: `src/replay.rs`
- Deterministic process fixtures: `examples/`
- End-to-end and conformance tests: `tests/`

## Good First Issue Workflow

1. Pick an issue labeled `good first issue`.
2. Reproduce the current behavior with a focused command or test.
3. Add or update the smallest test that captures the expected behavior.
4. Make the code change.
5. Run the relevant focused test, then the full CI check set before opening a
   pull request.

Keep new behavior local-first and zero-setup by default. Features that use a
network service or model should be optional and should have a graceful fallback.
