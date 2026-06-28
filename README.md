<div align="center">

<img src="logo.png" alt="Delphin logo" width="160"/>

# Delphin 🐬

**A duplex companion for AI agent CLIs**, written in Rust.

</div>

*Delphin* — from the Greek **delphís** (dolphin), with a fin tipped to **Delphi**.
Dolphins swim in pods, bow-ride alongside ships, and talk in a constant stream of
clicks. Delphin does the same for your AI agent: it stays alongside while the
agent thinks, lets you keep talking, and remembers every word.

## The problem

Talking to an AI agent (Claude, Codex, …) is **half-duplex**. While it's thinking
you either wait, or interrupt and lose the in-flight work. There's no natural
"let me add one more thing" or "wait, wrong file" mid-thought.

## What Delphin does

Delphin runs the agent in a **PTY** and mirrors its output, so it looks normal —
but:

- **Keep typing while the agent thinks.** Extra prompts go into a **queue** and
  are released one at a time as the agent finishes.
- A pluggable **arbiter** decides what each new prompt deserves:
  - **send now** — agent is idle, forward immediately;
  - **enqueue** — agent is busy and the prompt can wait (the default — a
    half-finished thought is worth protecting);
  - **interrupt** — agent is busy but you signalled urgency ("stop", "wait",
    "no", "actually", …), so it barges in.
- **It remembers.** Every prompt, the arbiter's verdict, and the agent's reply
  are written to a local SQLite file (`agent_turns`) — your conversation history,
  on your machine, nowhere else.

```
        you type ──────────────┐
                               ▼
                        ┌─────────────┐  busy?   ┌───────────┐
   agent output ◀───────│  supervisor │─────────▶│  arbiter  │
   (mirrored to you)    │ (PTY + idle │          └───────────┘
                        │  detection) │  verdict: send / enqueue / interrupt
                        └─────┬───────┘
                              ▼                  every turn ──▶ SQLite (agent_turns)
                        ┌───────────┐
                        │   queue   │ (drained one prompt per idle)
                        └───────────┘
```

## Why it matters

AI coding agents are becoming long-running collaborators rather than single-shot
assistants. As they take on larger tasks—indexing repositories, generating code,
running tests, or refactoring—they spend much of their time thinking while the
user waits. Current CLIs treat interaction as half-duplex: either the agent is
working or the human is speaking, but not both. Delphin removes this artificial
constraint. By allowing users to continue providing context without discarding
the agent's ongoing work, it makes conversations with AI agents feel closer to
conversations with people. At the same time, its local SQLite memory creates a
transparent, inspectable record of every interaction that remains entirely under
the user's control. Delphin doesn't replace an AI agent; it improves the
communication layer around one, making long-running human–AI collaboration
smoother, less interruptive, and more resilient.

## Try it (no real model needed)

```bash
cargo build
cargo run -- --interrupt ctrl-c -- bash examples/mock-agent.sh
```

While the mock is "thinking" (printing dots):
- type `also add logging` → it **queues**, sent when the mock finishes;
- type `stop wrong thing` → it **interrupts** and sends your line.

## Use it with a real agent

```bash
cargo run --release -- -- claude
cargo run --release -- --interrupt esc -- codex
```

## Memory

By default Delphin remembers into `<data_local>/Delphin/delphin.sqlite3`. Inspect it:

```bash
sqlite3 ~/Library/Application\ Support/Delphin/delphin.sqlite3 \
  "SELECT direction, verdict, substr(text,1,60) FROM agent_turns ORDER BY id DESC LIMIT 20;"
```

Point it at a different database to let Delphin *accompany* another system's
memory (companionship by choice, not dependency):

```bash
cargo run -- --db /path/to/other.sqlite3 -- claude
```

Or turn memory off entirely with `--no-log`.

## Options

```
--idle-ms N        silence (ms) before the agent is considered idle [800]
--tick-ms N        idle-detector tick interval (ms) [150]
--submit-newline   submit prompts with "\n" instead of "\r"
--interrupt KIND   esc | double-esc | ctrl-c | none | <literal> [esc]
--db PATH          remember into this SQLite file instead of the default
--no-log           do not remember the conversation
```

## Use it as a Claude Code or Codex skill

Delphin ships skills for both agents so you can recall, run, or set it up from
inside your assistant (e.g. *"recall my last Delphin session"*, *"search Delphin
memory for the migration"*).

**Claude Code** — copy the skill into your skills directory:
```bash
# project-scoped:
cp -r .claude/skills/delphin <your-project>/.claude/skills/
# or user-wide:
cp -r .claude/skills/delphin ~/.claude/skills/
```
Then it triggers automatically, or invoke it with `/delphin`.

**Codex** — copy the prompt into your Codex prompts directory:
```bash
cp .codex/prompts/delphin.md ~/.codex/prompts/
```
Then run `/delphin` (e.g. `/delphin recall migration`).

Both skills know how to query Delphin's `agent_turns` memory and how to build/run
the wrapper.

## Honest caveats (v1)

- **Idle = silence.** "Is the agent thinking?" is inferred from output silence
  (`--idle-ms`); there's no portable readiness API, so it needs per-agent tuning.
- **Interrupting is agent-specific.** ESC stops Claude Code; many CLIs use Ctrl-C.
  Set `--interrupt` accordingly; `none` gives queue-only mode.
- **Line-oriented input.** Delphin reads whole lines; rich TUIs may render
  imperfectly. Line-based agents work cleanly.

## Layout

| File | Role |
|---|---|
| `src/main.rs` | CLI parsing, wiring |
| `src/supervisor.rs` | PTY spawn, idle detection, event loop |
| `src/arbiter.rs` | `Arbiter` trait + default heuristic (+ tests) |
| `src/queue.rs` | prompt FIFO (+ tests) |
| `src/memory.rs` | self-contained SQLite log (+ tests) |
| `examples/mock-agent.sh` | fake thinking agent for testing |

## Tests

```bash
cargo test
```

## License

MIT
