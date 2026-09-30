# Ecosystem

Delphin is one of a small set of related, local-first projects. They are separate
repositories that **refer to each other** and compose cleanly — the memory and the
conversation belong to *you*, not to any one model.

| Project | Role | Repo |
|---|---|---|
| **Delphin** 🐬 | **Communication** — a duplex wrapper for AI agent CLIs: keep talking while the agent thinks; an arbiter decides interrupt-vs-wait. (this repo) | https://github.com/wuisabel-gif/Delphin |
| **MemoryWhale** 🐋 | **Memory** — an inspectable memory OS: capture, retrieve with explanations, forget. | https://github.com/wuisabel-gif/MemWhale |

## How they fit together

```
        you ⇄ AI agent
            │ (Delphin makes the conversation duplex)
            ▼
        Delphin  ──writes conversation turns──▶  MemoryWhale
        (communication)                          (memory: recall + explain)
```

- **Delphin** smooths the live conversation and records every turn.
- **MemoryWhale** stores, ranks, and **explains** what's worth remembering.

## Wiring them together (optional)

Delphin keeps its own local memory by default. With MemoryWhale 0.15 or newer
installed, `mw integrate delphin` checks both sides and prints the command:

```bash
delphin --memorywhale -- claude
```

That does two things:

- **Records through MemoryWhale.** Delphin streams each turn to `mw turns`, so
  MemoryWhale redacts secrets, applies its capture rules, and owns the schema.
  No database path to type. MemoryWhale's **Recall** panel then searches those
  turns alongside your notes and terminal commands.
- **Warns you live.** When an error line scrolls past, Delphin asks `mw hint`
  whether MemoryWhale has seen it. If so, a one-line hint appears while the
  agent is still working:

  ```text
  🐬 MemoryWhale: seen 2 times, the fix was: xcode-select --install
  ```

Set `memorywhale = true` in `config.toml` to make it the default.

## Shared database contract

The integration owns one table, `agent_turns`. Delphin writes it and MemoryWhale
retrieval reads the `id`, `ts`, `direction`, and `text` columns. Delphin also
uses `session_id`, `verdict`, `cwd`, and the additive `turn_group_id` column.
Neither project should repurpose those names with incompatible types or
constraints.

With `--memorywhale`, MemoryWhale writes this table itself. The rules below
apply to the older route, `--db` pointed at MemoryWhale's database file, which
still works. When `--db` points at an existing database, Delphin:

- enables WAL mode and a three-second busy timeout for concurrent access;
- creates `agent_turns` when it is absent;
- adds `turn_group_id` when opening the older shared schema;
- validates all required columns before recording begins; and
- leaves unrelated MemoryWhale tables and schema-version metadata untouched.

An incompatible `agent_turns` table fails at startup with the missing columns
listed, rather than allowing the session to run while silently losing every
memory write.

## Naming

Both are cetaceans on purpose: **Delphin** (the dolphin) for communication,
**MemoryWhale** for memory — related pieces of infrastructure, not isolated tools.
