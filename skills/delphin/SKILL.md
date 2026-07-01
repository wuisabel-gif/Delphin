---
name: delphin
description: Set up, run, or recall conversations from Delphin — the duplex wrapper for AI agent CLIs that lets you keep typing while the agent thinks (prompts queue or stream in live mode; a heuristic arbiter decides send-now / enqueue / stream / interrupt) and remembers every turn in a local SQLite database. Use when the user wants to build/install/run Delphin, wrap an agent (claude/codex) with it, or recall/search past Delphin sessions and their queued, streamed, or interrupted prompts.
---

# Delphin

Delphin (`delphin`) wraps an AI agent CLI in a PTY so the user can keep typing
while it thinks: extra prompts queue, a pluggable heuristic arbiter decides
**send-now / enqueue / stream / interrupt**, and every turn is logged to a local SQLite
database. Repo: https://github.com/wuisabel-gif/Delphin

## Recall past conversations (most common use)

The memory database lives at:
- **macOS:** `~/Library/Application Support/Delphin/delphin.sqlite3`
- **Linux:** `~/.local/share/Delphin/delphin.sqlite3`
- or wherever the user pointed `--db`.

Table `agent_turns` — columns: `id`, `session_id`, `ts`, `direction`
(`user` | `agent` | `system`), `verdict` (`send_now` | `enqueue` | `stream` | `interrupt` | NULL),
`text`, `cwd`.

Set `DB="<path above>"`, then:

List recent sessions:
```bash
sqlite3 "$DB" "SELECT session_id, count(*) AS turns, min(ts) AS started FROM agent_turns GROUP BY session_id ORDER BY max(id) DESC LIMIT 10;"
```
Show one session's turns in order:
```bash
sqlite3 "$DB" "SELECT direction, COALESCE(verdict,''), substr(text,1,200) FROM agent_turns WHERE session_id='<SESSION>' ORDER BY id;"
```
Search across everything:
```bash
sqlite3 "$DB" "SELECT session_id, direction, text FROM agent_turns WHERE text LIKE '%<TERM>%' ORDER BY id DESC LIMIT 30;"
```
Present the user's own words **verbatim** — do not paraphrase stored turns.

## Build & run

```bash
git clone https://github.com/wuisabel-gif/Delphin && cd Delphin
cargo build --release
./target/release/delphin -- claude                                   # wrap a real agent
./target/release/delphin --interrupt ctrl-c -- bash examples/mock-agent.sh   # no-model demo
```

Flags: `--idle-ms N`, `--tick-ms N`, `--submit-newline`,
`--live`, `--interrupt {esc|double-esc|ctrl-c|none|<literal>}`, `--db PATH`, `--no-log`.

While the agent is thinking, ordinary prompts queue; a line containing an
urgency word (`stop`, `wait`, `no`, `actually`, …) interrupts instead.
With `--live`, ordinary busy prompts stream immediately into the wrapped PTY
instead of queueing; this is most useful for rich TUI agents that accept
type-ahead while generating.

## Notes / gotchas

- "Thinking" is inferred from output **silence** (`--idle-ms`, default 800ms) —
  tune per agent if it releases prompts mid-thought or feels sluggish.
- The interrupt key is **agent-specific**: ESC stops Claude Code; many CLIs use
  Ctrl-C. Set `--interrupt` accordingly; `none` = queue-only mode.
- `--live` only guarantees immediate delivery to the PTY. The wrapped agent
  decides whether that input is visible or useful mid-generation.
- Use `--db <other.sqlite3>` to make Delphin write into another tool's database
  (e.g. share memory with a companion app).
