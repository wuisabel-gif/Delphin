---
description: Recall, run, or set up Delphin — the duplex agent-CLI wrapper that remembers conversations.
argument-hint: "[recall <term> | run | setup]"
---

You are operating the **Delphin** skill. Delphin (`delphin`) wraps an AI agent
CLI so the user can keep typing while it thinks: prompts queue, a heuristic
arbiter decides **send-now / enqueue / interrupt**, and every turn is logged to a
local SQLite database. Repo: https://github.com/wuisabel-gif/Delphin

Interpret the user's request — `$ARGUMENTS` — and act:

## Recall (default when given a search term or a session)

Memory database:
- macOS: `~/Library/Application Support/Delphin/delphin.sqlite3`
- Linux: `~/.local/share/Delphin/delphin.sqlite3`

Table `agent_turns(id, session_id, ts, direction['user'|'agent'|'system'],
verdict['send_now'|'enqueue'|'interrupt'|NULL], text, cwd)`. Use sqlite3:

```bash
DB="$HOME/Library/Application Support/Delphin/delphin.sqlite3"   # or the Linux path
# recent sessions:
sqlite3 "$DB" "SELECT session_id, count(*), min(ts) FROM agent_turns GROUP BY session_id ORDER BY max(id) DESC LIMIT 10;"
# search:
sqlite3 "$DB" "SELECT session_id, direction, text FROM agent_turns WHERE text LIKE '%TERM%' ORDER BY id DESC LIMIT 30;"
# one session in order:
sqlite3 "$DB" "SELECT direction, COALESCE(verdict,''), substr(text,1,200) FROM agent_turns WHERE session_id='SESSION' ORDER BY id;"
```
Quote the user's stored words **verbatim**; never paraphrase a remembered turn.

## Run / setup

```bash
git clone https://github.com/wuisabel-gif/Delphin && cd Delphin
cargo build --release
./target/release/delphin -- codex                                    # wrap a real agent
./target/release/delphin --interrupt ctrl-c -- bash examples/mock-agent.sh   # demo
```
Flags: `--idle-ms`, `--tick-ms`, `--submit-newline`,
`--interrupt {esc|double-esc|ctrl-c|none|<literal>}`, `--db PATH`, `--no-log`.

Notes: "thinking" is inferred from output silence (`--idle-ms`); the interrupt
key is agent-specific (ESC for Claude Code, Ctrl-C for many CLIs).
