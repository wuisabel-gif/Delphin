<div align="center">

<img src="logo.png" alt="Delphin logo" width="160"/>

# Delphin 🐬

**A duplex companion for AI agent CLIs**, written in Rust.

[![Claude Code skill](https://img.shields.io/badge/Claude_Code-skill_available-D97757?logo=anthropic&logoColor=white)](.claude/skills/delphin/SKILL.md)
[![Codex skill](https://img.shields.io/badge/Codex-skill_available-412991?logo=openai&logoColor=white)](.codex/prompts/delphin.md)
[![Rust](https://img.shields.io/badge/Rust-CE422B?logo=rust&logoColor=white)](Cargo.toml)
[![License: MIT](https://img.shields.io/badge/license-MIT-3da639)](#license)
[![Sibling: MemoryWhale](https://img.shields.io/badge/sibling-MemoryWhale%20%F0%9F%90%8B-2b43dd)](https://github.com/wuisabel-gif/MemWhale)

</div>

> 🐋 **Sibling project:** [MemoryWhale](https://github.com/wuisabel-gif/MemWhale) — the **memory** layer. Delphin handles *communication* (talk while the agent thinks); MemoryWhale handles *memory* (recall with explanations). They refer to each other — see [ECOSYSTEM.md](ECOSYSTEM.md). Point Delphin at MemoryWhale's database with `--db` to feed your conversations into it.

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
  - **stream** — in `--live` mode, agent is busy but the prompt should be
    forwarded immediately without interrupting;
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
                        │  detection) │  verdict: send / enqueue / stream / interrupt
                        └─────┬───────┘
                              ▼                  every turn ──▶ SQLite (agent_turns)
                        ┌───────────┐
                        │   queue   │ (drained one prompt per idle)
                        └───────────┘
```

## Motivation

The idea for Delphin came after watching the release of **Interaction Models** by
Thinking Machines Lab, led by Mira Murati, in May 2026. Their central argument was
that today's AI interactions are constrained by a turn-based interface: while the
user is speaking, the model waits; while the model is generating, it stops
perceiving new information. They argued that this creates a communication
bottleneck and that future AI systems should support continuous, duplex
interaction rather than alternating turns. *(Thinking Machines Lab)*

That immediately reminded me of an experience I had using Claude Code. I had asked
it to work on a development task, and it spent nearly fifteen minutes implementing
something that turned out not to be what I intended. The problem wasn't simply
that it misunderstood the request—misunderstandings happen in any collaboration.
The frustrating part was that there was no natural way to say, "Wait, that's not
what I meant," without interrupting the entire session and discarding the work
already in progress. Looking back, I also realized that the agent never paused to
ask the clarifying question that a human collaborator would likely have asked
before investing fifteen minutes in the wrong direction.

That experience convinced me that part of the problem lies not in the model's
reasoning, but in the interaction model surrounding it. Delphin explores that idea
from a systems perspective. Instead of training a new foundation model, it wraps
today's AI agent CLIs and upgrades them from half-duplex to a more duplex style of
interaction: you can continue communicating while the agent works, and a
supervisor decides whether each new message should be delivered immediately,
queued until the agent finishes, or used to interrupt the current task. It is an
attempt to improve human–AI collaboration today, using the agents we already have.

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

For rich TUI agents that already accept type-ahead while generating, try live
mode:

```bash
cargo run --release -- --live --interrupt esc -- claude
cargo run --release -- --live --interrupt esc -- codex
```

`--live` streams ordinary busy prompts straight into the wrapped PTY instead of
holding them in Delphin's FIFO. The agent still decides whether it can actually
use that input mid-generation: rich TUIs may place it in their compose buffer,
while line-oriented programs usually just receive it on their next `read`.

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
--live             stream busy prompts immediately instead of queueing
--interrupt KIND   esc | double-esc | ctrl-c | none | <literal> [esc]
--arbiter KIND     heuristic | question [heuristic]
--ready MARKER     output ending with MARKER means the agent is idle (repeatable)
--db PATH          remember into this SQLite file instead of the default
--no-log           do not remember the conversation
```

### Arbiter policies

The policy that decides *wait vs interrupt* is swappable with `--arbiter`:

- **`heuristic`** (default) — protect the in-flight work; only an urgency word
  ("stop", "wait", "no", "actually", …) interrupts.
- **`question`** — also interrupt on **questions** (ends with "?", or starts with
  who/what/why/how/…), since a question usually needs answering before the
  current work is useful. Plain instructions still queue.

### Smarter idle detection

By default "idle" is inferred from output silence (`--idle-ms`). If your agent
prints a recognizable prompt when it's waiting, tell Delphin and it'll go idle
the moment that prompt appears instead of waiting out the timer:

```bash
delphin --ready 'you> ' --ready '❯ ' -- bash examples/mock-agent.sh
```

### Config file

Defaults can live in `./.delphin.toml` (or `<config-dir>/delphin/config.toml`);
CLI flags override them:

```toml
idle_ms = 1000
interrupt = "ctrl-c"
arbiter = "question"
ready_markers = ["you> ", "❯ "]
```

## Recall — query your conversation memory

```bash
delphin recall postgres                 # search remembered turns
delphin recall                          # most recent turns
delphin recall --db /path/to.sqlite3 --limit 50 "auth"
```

Each line shows the date, session, direction, the arbiter's verdict, and the
remembered text.

## Use it as a Claude Code or Codex skill

Delphin ships skills for both agents so you can recall, run, or set it up from
inside your assistant (e.g. *"recall my last Delphin session"*, *"search Delphin
memory for the migration"*).

**Claude Code — install as a plugin (recommended):** this repo is a Claude Code
marketplace, so you can install Delphin's skill directly:
```text
/plugin marketplace add wuisabel-gif/Delphin
/plugin install delphin@delphin
```
Then it triggers automatically, or invoke it with `/delphin`.

**Claude Code — manual copy (alternative):**
```bash
cp -r skills/delphin ~/.claude/skills/        # user-wide
# or: cp -r skills/delphin <your-project>/.claude/skills/
```

**Codex** — copy the prompt into your Codex prompts directory:
```bash
cp .codex/prompts/delphin.md ~/.codex/prompts/
```
Then run `/delphin` (e.g. `/delphin recall migration`).

Both skills know how to query Delphin's `agent_turns` memory and how to build/run
the wrapper. Plugin manifests live in [`.claude-plugin/`](.claude-plugin/).

## Honest caveats (v1)

- **Idle = silence.** "Is the agent thinking?" is inferred from output silence
  (`--idle-ms`); there's no portable readiness API, so it needs per-agent tuning.
- **Interrupting is agent-specific.** ESC stops Claude Code; many CLIs use Ctrl-C.
  Set `--interrupt` accordingly; `none` gives queue-only mode.
- **Line-oriented input.** Delphin reads whole lines; rich TUIs may render
  imperfectly. Line-based agents work cleanly.
- **Live mode depends on the wrapped agent.** `--live` delivers input bytes
  immediately, but only agents that accept type-ahead or mid-generation input
  can make that feel genuinely simultaneous.

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
