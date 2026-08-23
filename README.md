<div align="center">

<img src="https://raw.githubusercontent.com/wuisabel-gif/Delphin/main/logo.png" alt="Delphin logo" width="160"/>

# Delphin 🐬

> **Install note:** crates.io may still serve an older release than this repo (workspace `Cargo.toml` is currently `0.3.0`). Prefer building from source (`cargo install --path .` or `cargo install --git https://github.com/wuisabel-gif/Delphin`) until the crates.io package is republished.

**A duplex companion for AI agent CLIs**, written in Rust.

[![Claude Code skill](https://img.shields.io/badge/Claude_Code-skill_available-D97757?logo=anthropic&logoColor=white)](skills/delphin/SKILL.md)
[![Codex skill](https://img.shields.io/badge/Codex-skill_available-412991?logo=openai&logoColor=white)](.codex/prompts/delphin.md)
[![Rust](https://img.shields.io/badge/Rust-CE422B?logo=rust&logoColor=white)](Cargo.toml)
[![License: MIT](https://img.shields.io/badge/license-MIT-3da639)](#license)
[![Sibling: MemoryWhale](https://img.shields.io/badge/sibling-MemoryWhale%20%F0%9F%90%8B-2b43dd)](https://github.com/wuisabel-gif/MemWhale)

<img src="https://raw.githubusercontent.com/wuisabel-gif/Delphin/main/demo.gif" alt="Delphin demo: keep typing while the agent thinks — prompts queue, 'stop' barges in, every turn is remembered" width="760"/>

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
    "cancel", "halt", …), so it barges in.
- **It remembers.** Every prompt, the arbiter's verdict, and the agent's reply
  are written to a local SQLite file (`agent_turns`) — your conversation history,
  on your machine, nowhere else.

> **Honest scope:** this is *asynchronous handoff*, not simultaneous listening.
> Your words reach the agent sooner (queued, streamed, or as a barge-in) instead
> of after you wait — but the agent still perceives them between turns, not
> mid-token. Delphin makes "talk while it thinks" ergonomic on today's CLIs; it
> doesn't make the model itself duplex.

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

## Install

```bash
brew install wuisabel-gif/delphin/delphin   # Homebrew
cargo install delphin                        # crates.io
```

Or from source: `git clone https://github.com/wuisabel-gif/Delphin && cd Delphin && cargo install --path .`

### Supported platforms

Delphin is continuously tested on current Linux and macOS runners. Its
conformance suite exercises Unix PTYs and shell-based fixtures on both systems.
Windows is not currently part of the supported or tested platform matrix.

## Try it (no real model needed)

```bash
delphin --interrupt ctrl-c -- bash examples/mock-agent.sh
```

While the mock is "thinking" (printing dots):
- type `also add logging` → it **queues**, sent when the mock finishes;
- type `stop wrong thing` → it **interrupts** and sends your line.

For rich TUI agents that already accept type-ahead while generating, try live
mode:

```bash
delphin --live --interrupt esc -- claude
delphin --live --interrupt esc -- codex
```

`--live` streams ordinary busy prompts straight into the wrapped PTY instead of
holding them in Delphin's FIFO. The agent still decides whether it can actually
use that input mid-generation: rich TUIs may place it in their compose buffer,
while line-oriented programs usually just receive it on their next `read`.

## Use it with a real agent

```bash
delphin --agent claude -- claude    # esc-interrupt + live, tuned for Claude Code
delphin --agent codex -- codex
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
delphin --db /path/to/other.sqlite3 -- claude
```

Or turn memory off entirely with `--no-log`.

### Privacy and retention

Memory includes complete prompts, arbiter decisions, supervisor events, and
terminal output. That can include source code, file paths, credentials printed by
a command, or secrets pasted into a prompt. Delphin does not upload this data,
redact it, or delete it automatically.

- Use `--no-log` for sensitive sessions.
- Newly created database files use owner-only permissions on Unix (`0600`).
- Existing databases passed through `--db` keep their current permissions.
- Backups and filesystem snapshots may retain copies after the live database is
  removed.

To erase all Delphin memory, first stop every Delphin process and then delete the
database path printed in the startup banner. SQLite may use adjacent `-wal` and
`-shm` files while a process is running, which is why deletion should only happen
after the process exits.

## Options

```
--idle-ms N        silence (ms) before the agent is considered idle [800]
--min-busy-ms N    minimum busy time before silence counts as idle [0]
--tick-ms N        idle-detector tick interval (ms) [150]
--submit-newline   submit prompts with "\n" instead of "\r"
--live             stream busy prompts immediately instead of queueing
--passthrough      forward raw terminal input byte-for-byte (Unix)
--interrupt KIND   esc | double-esc | ctrl-c | none | <literal> [esc]
--agent KIND       preset interrupt+live defaults for claude | codex | generic
--interrupt-word W add W to the urgency words that barge in (repeatable)
--arbiter KIND     heuristic | question [heuristic]
--ready MARKER     output ending with MARKER means the agent is idle (repeatable)
--db PATH          remember into this SQLite file instead of the default
--no-log           do not remember the conversation
```

### Agent presets

Skip tuning flags one by one — `--agent` sets a sensible baseline for a known
agent (any explicit flag still overrides it):

```bash
delphin --agent claude -- claude     # esc interrupt + live type-ahead
delphin --agent generic -- some-repl # ctrl-c interrupt, queue mode
```

`claude`/`codex` → `--interrupt esc --live`; `generic` → `--interrupt ctrl-c`,
queue mode. Starting points, not gospel — override any of them explicitly.

### Raw terminal passthrough

Line-based prompt routing is the default because Delphin needs complete prompts
to queue and arbitrate them. For a full-screen TUI that depends on arrow keys,
completion menus, bracketed paste, mouse input, or other raw terminal controls,
use byte-for-byte passthrough:

```bash
delphin --passthrough -- some-tui
```

Passthrough mode temporarily places the Unix terminal in raw mode and restores
its original settings on exit. Because input is no longer divided into prompts,
queueing, user-prompt memory, and arbiter decisions are disabled in this mode;
agent output is still mirrored and remembered.

### Arbiter policies

The policy that decides *wait vs interrupt* is swappable with `--arbiter`:

- **`heuristic`** (default) — protect the in-flight work; only an urgency word
  ("stop", "wait", "cancel", "halt", …) interrupts. Add your own with
  `--interrupt-word` (repeatable) or `interrupt_keywords` in the config file.
- **`question`** — also interrupt on **questions** (ends with "?", or starts with
  who/what/why/how/…), since a question usually needs answering before the
  current work is useful. Plain instructions still queue.

### Custom arbiter policies

Delphin also exposes a Rust library target. Other crates can implement
`delphin::arbiter::Arbiter` and pass the policy to
`delphin::supervisor::run`, using the same `Decision` and `Verdict` types as the
built-in policies.

See [`examples/custom_arbiter.rs`](examples/custom_arbiter.rs) for a complete
policy and run it with:

```bash
cargo run --example custom_arbiter
```

### Smarter idle detection

By default "idle" is inferred from output silence (`--idle-ms`). If your agent
prints a recognizable prompt when it's waiting, tell Delphin and it'll go idle
the moment that prompt appears instead of waiting out the timer:

```bash
delphin --ready 'you> ' --ready '❯ ' -- bash examples/mock-agent.sh
```

Two guards make silence-based detection less twitchy:

- **`--min-busy-ms N`** — a silence gap doesn't count as idle until the agent has
  been busy at least this long, so a brief stall right after a prompt can't flip
  idle instantly.
- **Mid-line guard (always on)** — if the agent's output ends mid-line it's
  probably still drawing, so Delphin waits a few more idle windows before
  releasing. It's bounded, so an agent that simply ends without a newline can't
  wedge the queue.

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

## Replay — check a different policy against real history

`delphin replay` re-runs any arbiter over your recorded conversations and
reports where it would have decided differently — so you can check a new
policy against real usage before it ever runs live:

```bash
delphin replay --arbiter question   # would the question arbiter interrupt more?
delphin replay --session <id>        # restrict to one session
```

<img src="https://raw.githubusercontent.com/wuisabel-gif/Delphin/main/demo-replay.gif" alt="Delphin replay demo: a question queued under the heuristic arbiter is shown as an interrupt under the question arbiter" width="760"/>

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

## Layout

| File | Role |
|---|---|
| `src/main.rs` | CLI parsing, wiring |
| `src/config.rs` | optional `.delphin.toml` defaults (+ tests) |
| `src/supervisor.rs` | PTY spawn, idle detection, event loop (+ tests) |
| `src/arbiter.rs` | `Arbiter` trait + default heuristic (+ tests) |
| `src/queue.rs` | prompt FIFO (+ tests) |
| `src/memory.rs` | self-contained SQLite log (+ tests) |
| `src/replay.rs` | re-run an arbiter over recorded history (+ tests) |
| `examples/mock-agent.sh` | fake thinking agent for testing |
| `tests/` | end-to-end tests against the mock agent |

## Tests

```bash
cargo test
```

## License

MIT
