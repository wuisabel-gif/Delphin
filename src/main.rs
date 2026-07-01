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
//!
//! Run `delphin --help` for the full option list.

mod arbiter;
mod config;
mod memory;
mod queue;
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

OPTIONS:
    --idle-ms N        silence (ms) before the agent is considered idle [800]
    --tick-ms N        idle-detector tick interval (ms) [150]
    --submit-newline   submit prompts with \"\\n\" instead of \"\\r\"
    --live             stream busy prompts immediately instead of queueing
    --interrupt KIND   esc | double-esc | ctrl-c | none | <literal> [esc]
    --arbiter KIND     heuristic | question [heuristic]
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

    let (cfg, cfg_src) = Config::load();

    let split = args.iter().position(|a| a == "--");
    let (ours, agent_cmd): (&[String], Vec<String>) = match split {
        Some(i) => (&args[..i], args[i + 1..].to_vec()),
        None => (&args[..], Vec::new()),
    };

    // Start from config (or built-in defaults); CLI flags override.
    let mut idle_ms = cfg.idle_ms;
    let mut tick_ms = cfg.tick_ms;
    let mut submit_newline = cfg.submit_newline;
    let mut interrupt = cfg.interrupt.clone();
    let mut arbiter_name = cfg.arbiter.clone();
    let mut live = cfg.live;
    let mut ready_markers = cfg.ready_markers.clone();
    let mut logging = cfg.log;
    let mut db: Option<PathBuf> = None;

    let mut it = ours.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--idle-ms" => idle_ms = parse_num(it.next(), "--idle-ms")?,
            "--tick-ms" => tick_ms = parse_num(it.next(), "--tick-ms")?,
            "--submit-newline" => submit_newline = true,
            "--live" => live = true,
            "--interrupt" => interrupt = next_val(it.next(), "--interrupt")?,
            "--arbiter" => arbiter_name = next_val(it.next(), "--arbiter")?,
            "--ready" => ready_markers.push(next_val(it.next(), "--ready")?),
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

    let arbiter = build_arbiter(arbiter_kind, cfg.interrupt_keywords.clone(), live);

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
        let text: String = h.text.replace('\n', " ").chars().take(100).collect();
        println!("{date}  ({session})  {:<7}{verdict}  {text}", h.direction);
    }
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
