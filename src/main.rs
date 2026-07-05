//! delphin — a duplex companion for AI agent CLIs.
//!
//! Like a dolphin alongside the ship, delphin stays with you while the agent
//! thinks: keep typing, prompts queue, and a pluggable arbiter decides whether a
//! new prompt should wait or interrupt. The whole conversation is remembered in a
//! local SQLite file.
//!
//! Usage:
//!   delphin [options] -- <agent-command> [args...]
//!   delphin recall [--db PATH] [--limit N] [query...]
//!   delphin replay [--db PATH] [--session ID] [--arbiter KIND]
//!
//! Run `delphin --help` for the full option list.

mod arbiter;
mod config;
mod memory;
mod queue;
mod replay;
mod supervisor;

use std::path::PathBuf;
use std::process::ExitCode;

use arbiter::{build_arbiter, ArbiterKind};
use chrono::Utc;
use config::Config;
use memory::MemoryLog;
use supervisor::Settings;

const HELP: &str = "\
delphin — a duplex companion for AI agent CLIs

USAGE:
    delphin [options] -- <agent-command> [args...]
    delphin recall [--db PATH] [--limit N] [query...]
    delphin replay [--db PATH] [--session ID] [--arbiter KIND] [--interrupt-word W]...

OPTIONS:
    --idle-ms N        silence (ms) before the agent is considered idle [800]
    --min-busy-ms N    minimum busy time before silence counts as idle [0]
    --tick-ms N        idle-detector tick interval (ms) [150]
    --submit-newline   submit prompts with \"\\n\" instead of \"\\r\"
    --live             stream busy prompts immediately instead of queueing
    --interrupt KIND   esc | double-esc | ctrl-c | none | <literal> [esc]
    --agent KIND       preset interrupt+live defaults for claude | codex | generic
    --arbiter KIND     heuristic | question [heuristic]
    --interrupt-word W add W to the urgency words that barge in (repeatable)
    --ready MARKER     output ending with MARKER means the agent is idle
                       (repeatable; e.g. --ready 'you> ')
    --db PATH          remember into this SQLite file instead of the default
    --no-log           do not remember the conversation
    -h, --help         show this help

Config: defaults are read from ./.delphin.toml or <config-dir>/delphin/config.toml;
CLI flags override them.

EVERYTHING after `--` is the agent command to wrap.

EXAMPLES:
    delphin -- claude
    delphin --arbiter question --interrupt ctrl-c -- bash examples/mock-agent.sh
    delphin recall postgres
    delphin replay --arbiter question
";

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("delphin: error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return Ok(());
    }

    // Subcommand: query the conversation memory.
    if args.first().map(String::as_str) == Some("recall") {
        return run_recall(&args[1..]);
    }

    // Subcommand: replay history through an arbiter, compare vs what happened.
    if args.first().map(String::as_str) == Some("replay") {
        return run_replay(&args[1..]);
    }

    let (cfg, cfg_src) = Config::load();

    let split = args.iter().position(|a| a == "--");
    let (ours, agent_cmd): (&[String], Vec<String>) = match split {
        Some(i) => (&args[..i], args[i + 1..].to_vec()),
        None => (&args[..], Vec::new()),
    };

    // Start from config (or built-in defaults); CLI flags override.
    let mut idle_ms = cfg.idle_ms;
    let mut min_busy_ms = cfg.min_busy_ms;
    let mut tick_ms = cfg.tick_ms;
    let mut submit_newline = cfg.submit_newline;
    let mut interrupt = cfg.interrupt.clone();
    let mut arbiter_name = cfg.arbiter.clone();
    let mut live = cfg.live;
    let mut ready_markers = cfg.ready_markers.clone();
    let mut interrupt_keywords = cfg.interrupt_keywords.clone();
    let mut logging = cfg.log;
    let mut db: Option<PathBuf> = None;

    // An --agent preset is a convenience baseline; apply it before the flag loop
    // so any explicit flag below still wins regardless of argument order.
    if let Some(name) = flag_value(ours, "--agent") {
        let (preset_interrupt, preset_live) = agent_preset(&name).ok_or_else(|| {
            anyhow::anyhow!("unknown --agent '{name}' (use: claude | codex | generic)")
        })?;
        interrupt = preset_interrupt.to_string();
        live = preset_live;
    }

    let mut it = ours.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--idle-ms" => idle_ms = parse_num(it.next(), "--idle-ms")?,
            "--min-busy-ms" => min_busy_ms = parse_num(it.next(), "--min-busy-ms")?,
            "--tick-ms" => tick_ms = parse_num(it.next(), "--tick-ms")?,
            "--submit-newline" => submit_newline = true,
            "--live" => live = true,
            "--interrupt" => interrupt = next_val(it.next(), "--interrupt")?,
            "--agent" => {
                it.next(); // value already consumed by the pre-scan above
            }
            "--arbiter" => arbiter_name = next_val(it.next(), "--arbiter")?,
            "--ready" => ready_markers.push(next_val(it.next(), "--ready")?),
            "--interrupt-word" => interrupt_keywords.push(next_val(it.next(), "--interrupt-word")?),
            "--db" => db = Some(PathBuf::from(next_val(it.next(), "--db")?)),
            "--no-log" => logging = false,
            other => anyhow::bail!(
                "unknown option `{other}` (did you forget `--` before the agent command?)"
            ),
        }
    }

    if agent_cmd.is_empty() {
        eprint!("{HELP}");
        anyhow::bail!("no agent command provided");
    }

    let arbiter_kind = ArbiterKind::parse(&arbiter_name).ok_or_else(|| {
        anyhow::anyhow!("unknown --arbiter '{arbiter_name}' (use: heuristic | question)")
    })?;

    if let Some(src) = cfg_src {
        eprintln!(
            "\x1b[2m[delphin]\x1b[0m loaded config from {}",
            src.display()
        );
    }

    let settings = Settings {
        agent_command: agent_cmd,
        idle_after_ms: idle_ms,
        min_busy_ms,
        tick_ms,
        submit: if submit_newline {
            b"\n".to_vec()
        } else {
            b"\r".to_vec()
        },
        interrupt_bytes: interrupt_bytes(&interrupt),
        interrupt_label: interrupt,
        ready_markers,
        rows: 40,
        cols: 120,
    };

    let arbiter = build_arbiter(arbiter_kind, interrupt_keywords, live);

    let memlog = if logging {
        let session_id = format!(
            "delphin-{}-{}",
            Utc::now().format("%Y%m%dT%H%M%SZ"),
            std::process::id()
        );
        let cwd = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(str::to_string));
        match MemoryLog::open(session_id, cwd, db) {
            Ok(ml) => Some(ml),
            Err(e) => {
                eprintln!("delphin: memory disabled ({e:#})");
                None
            }
        }
    } else {
        None
    };

    supervisor::run(&settings, arbiter, memlog)
}

/// `delphin recall [--db PATH] [--limit N] [query...]` — search the conversation
/// memory and print matching turns (newest first).
fn run_recall(args: &[String]) -> anyhow::Result<()> {
    let mut db: Option<PathBuf> = None;
    let mut limit: usize = 20;
    let mut terms: Vec<String> = Vec::new();

    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--db" => db = Some(PathBuf::from(next_val(it.next(), "--db")?)),
            "--limit" => {
                limit = next_val(it.next(), "--limit")?
                    .parse()
                    .map_err(|_| anyhow::anyhow!("--limit must be a number"))?
            }
            other => terms.push(other.to_string()),
        }
    }

    let query = terms.join(" ");
    let hits = memory::search(db, &query, limit)?;
    if hits.is_empty() {
        if query.is_empty() {
            println!("(no remembered conversations yet)");
        } else {
            println!("(no memories matching {query:?})");
        }
        return Ok(());
    }
    for h in hits {
        let date = h.ts.split('T').next().unwrap_or(&h.ts);
        // Short session tag: the trailing pid of "delphin-<ts>-<pid>".
        let session = h.session_id.rsplit('-').next().unwrap_or(&h.session_id);
        let verdict = h
            .verdict
            .as_deref()
            .map(|v| format!(" [{v}]"))
            .unwrap_or_default();
        // Group tag ties a prompt to its release and the reply it triggered.
        let group = h
            .turn_group_id
            .map(|g| format!(" g{g}"))
            .unwrap_or_default();
        let text: String = h.text.replace('\n', " ").chars().take(100).collect();
        println!(
            "{date}  ({session}{group})  {:<7}{verdict}  {text}",
            h.direction
        );
    }
    Ok(())
}

/// `delphin replay [--db PATH] [--session ID] [--arbiter KIND] [--interrupt-word W]...`
/// — re-run an arbiter over real history and report where it disagrees with
/// what was actually decided at the time. See [`replay::replay`].
fn run_replay(args: &[String]) -> anyhow::Result<()> {
    let mut db: Option<PathBuf> = None;
    let mut session: Option<String> = None;
    let mut arbiter_name = "heuristic".to_string();
    let mut interrupt_keywords: Vec<String> = arbiter::DEFAULT_INTERRUPT_KEYWORDS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut live = false;

    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--db" => db = Some(PathBuf::from(next_val(it.next(), "--db")?)),
            "--session" => session = Some(next_val(it.next(), "--session")?),
            "--arbiter" => arbiter_name = next_val(it.next(), "--arbiter")?,
            "--interrupt-word" => interrupt_keywords.push(next_val(it.next(), "--interrupt-word")?),
            "--live" => live = true,
            other => anyhow::bail!("unknown replay option `{other}`"),
        }
    }

    let kind = ArbiterKind::parse(&arbiter_name).ok_or_else(|| {
        anyhow::anyhow!("unknown --arbiter '{arbiter_name}' (use: heuristic | question)")
    })?;
    let arbiter = build_arbiter(kind, interrupt_keywords, live);

    let turns = replay::replay(db, session.as_deref(), arbiter.as_ref())?;
    if turns.is_empty() {
        println!("(no user turns to replay yet — nothing logged, or the database doesn't exist)");
        return Ok(());
    }

    let mut agree = 0usize;
    for t in &turns {
        if t.agrees() {
            agree += 1;
        } else {
            let date = t.ts.split('T').next().unwrap_or(&t.ts);
            // Short session tag: the trailing pid of "delphin-<ts>-<pid>".
            let session = t.session_id.rsplit('-').next().unwrap_or(&t.session_id);
            let text: String = t.text.replace('\n', " ").chars().take(70).collect();
            println!(
                "{date}  ({session})  recorded={:<9} replayed={:<9}  {text}",
                t.recorded_verdict, t.replayed_verdict
            );
        }
    }
    println!(
        "\n{agree}/{} turns agree with `{}` ({:.0}%)",
        turns.len(),
        arbiter.name(),
        100.0 * agree as f64 / turns.len() as f64
    );
    Ok(())
}

fn next_val(v: Option<&String>, flag: &str) -> anyhow::Result<String> {
    v.cloned()
        .ok_or_else(|| anyhow::anyhow!("{flag} requires a value"))
}

fn parse_num(v: Option<&String>, flag: &str) -> anyhow::Result<u64> {
    v.ok_or_else(|| anyhow::anyhow!("{flag} requires a number"))?
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("{flag} must be a non-negative integer"))
}

fn interrupt_bytes(kind: &str) -> Vec<u8> {
    match kind {
        "esc" => vec![0x1b],
        "double-esc" => vec![0x1b, 0x1b],
        "ctrl-c" => vec![0x03],
        "none" => vec![],
        other => other.as_bytes().to_vec(),
    }
}

/// Value that follows `flag` in `args`, if present (first occurrence).
fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

/// Convenience baseline per wrapped agent: (interrupt key, live mode). These are
/// starting points, not gospel — any explicit flag overrides them.
// ponytail: two knobs that actually differ between agents (interrupt key and
// whether the TUI takes type-ahead); everything else stays on the shared default.
fn agent_preset(name: &str) -> Option<(&'static str, bool)> {
    match name.to_lowercase().as_str() {
        // rich TUIs that accept type-ahead and stop on ESC
        "claude" | "codex" => Some(("esc", true)),
        // safe default for a line-oriented tool: Ctrl-C, queue (no live)
        "generic" => Some(("ctrl-c", false)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::agent_preset;

    #[test]
    fn agent_presets() {
        assert_eq!(agent_preset("claude"), Some(("esc", true)));
        assert_eq!(agent_preset("CODEX"), Some(("esc", true))); // case-insensitive
        assert_eq!(agent_preset("generic"), Some(("ctrl-c", false)));
        assert_eq!(agent_preset("nope"), None);
    }
}
