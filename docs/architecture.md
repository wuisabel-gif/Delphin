# Architecture

Delphin is a Rust CLI that wraps another agent command in a PTY. Its core job is
to let the user keep typing while the wrapped agent is busy, then decide whether
each new prompt should be sent now, queued, streamed, or used as an interrupt.

## Runtime Flow

1. `src/main.rs` parses CLI flags and config, builds `supervisor::Settings`, and
   chooses an `Arbiter` implementation.
2. `src/supervisor.rs` opens a PTY, spawns the wrapped command, mirrors agent
   output to stdout, and reads user input from stdin.
3. Each completed user line becomes an `arbiter::Decision` with the current
   agent phase, busy elapsed time, and queue depth.
4. The selected arbiter returns a verdict:
   - `send_now`: write the prompt to the PTY immediately.
   - `enqueue`: store the prompt in the FIFO until the agent becomes idle.
   - `interrupt`: send the configured interrupt bytes, then send the new prompt.
   - `stream`: write the prompt while the agent is busy without interrupting.
5. When logging is enabled, `src/memory.rs` records user prompts, agent output,
   and supervisor events in the local SQLite `agent_turns` table.

## Modules

### `src/main.rs`

The binary entrypoint owns CLI parsing, agent presets, config loading, and
top-level command selection. It keeps the user-facing flag behavior close to the
place where defaults and presets are assembled.

### `src/supervisor.rs`

The supervisor owns the live PTY session. It is the largest runtime module
because it coordinates child process lifetime, terminal I/O, idle detection,
prompt routing, queue release, interrupt handling, resize propagation, and memory
flushes.

Idle detection is heuristic. Delphin can use silence windows, ready markers, a
minimum busy-time floor, and mid-line output guards to decide when it is safe to
release the next queued prompt.

### `src/arbiter.rs`

The arbiter layer owns prompt-routing policy. `Arbiter` is a trait so new
policies can be tested without forking the supervisor loop.

The default `HeuristicArbiter` protects the in-flight answer: busy prompts queue
unless they contain configured urgency words. `QuestionArbiter` is a stricter
variant that also treats questions as interrupt-worthy.

### `src/queue.rs`

The queue is a small FIFO for prompts that can wait. Each queued prompt carries a
group id so the original user row, later release event, and resulting agent reply
can stay linked in memory.

### `src/memory.rs`

The memory module owns the SQLite log. It creates or opens the database, validates
the `agent_turns` schema, writes user/agent/system rows, strips ANSI from stored
agent output, and exposes the default platform-local database path.

Memory is intentionally local and best-effort: logging failures are reported to
stderr but do not crash the live conversation.

### `src/replay.rs`

Replay reads existing `agent_turns` history, reconstructs enough runtime context
to rebuild arbiter decisions, and compares a chosen arbiter's verdicts with the
verdicts that were originally recorded. This lets policy changes be tested
against real sessions before they run live.

### `src/config.rs`

Config loading merges `.delphin.toml` values with CLI defaults and agent presets.
Keep config changes compatible with the default local-first, zero-setup path.

## Contributor Pointers

- CLI flag behavior usually starts in `src/main.rs`.
- Prompt-routing behavior usually belongs in `src/arbiter.rs` or
  `src/supervisor.rs`.
- Queue ordering and group-id behavior belongs in `src/queue.rs`.
- Stored conversation history belongs in `src/memory.rs`.
- Regression coverage for full PTY behavior usually belongs under `tests/`.
