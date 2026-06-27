//! delphin — a duplex companion for AI agent CLIs.
//!
//! Like the delphin orbiting alongside, delphin stays with you while the agent thinks:
//! keep typing, prompts queue, and a pluggable arbiter decides whether a new
//! prompt should wait or interrupt. The whole conversation is remembered in a
//! local SQLite file.
//!
//! Usage:
//!   delphin [options] -- <agent-command> [args...]
//!
//! Options:
//!   --idle-ms N        silence (ms) before the agent is considered idle [800]
//!   --tick-ms N        idle-detector tick interval (ms) [150]
//!   --submit-newline   submit prompts with "\n" instead of "\r"
//!   --interrupt KIND   esc | double-esc | ctrl-c | none | <literal> [esc]
//!   --db PATH          remember into this SQLite file instead of delphin's default
//!                      (e.g. point it at another app's DB to share memory)
//!   --no-log           do not remember the conversation
//!   -h, --help         show this help
//!
//! Examples:
//!   delphin -- claude
//!   delphin --interrupt ctrl-c -- bash examples/mock-agent.sh

mod arbiter;
mod memory;
mod queue;
mod supervisor;

use std::path::PathBuf;
use std::process::ExitCode;

use arbiter::{HeuristicArbiter, DEFAULT_INTERRUPT_KEYWORDS};
use chrono::Utc;
use memory::MemoryLog;
use supervisor::Settings;

const HELP: &str = "\
delphin — a duplex companion for AI agent CLIs

USAGE:
    delphin [options] -- <agent-command> [args...]

OPTIONS:
    --idle-ms N        silence (ms) before the agent is considered idle [800]
    --tick-ms N        idle-detector tick interval (ms) [150]
    --submit-newline   submit prompts with \"\\n\" instead of \"\\r\"
    --interrupt KIND   esc | double-esc | ctrl-c | none | <literal> [esc]
    --db PATH          remember into this SQLite file instead of delphin's default
    --no-log           do not remember the conversation
    -h, --help         show this help

EVERYTHING after `--` is the agent command to wrap.

EXAMPLES:
    delphin -- claude
    delphin --interrupt ctrl-c -- bash examples/mock-agent.sh
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

    let split = args.iter().position(|a| a == "--");
    let (ours, agent_cmd): (&[String], Vec<String>) = match split {
        Some(i) => (&args[..i], args[i + 1..].to_vec()),
        None => (&args[..], Vec::new()),
    };

    let mut idle_ms: u64 = 800;
    let mut tick_ms: u64 = 150;
    let mut submit_newline = false;
    let mut interrupt = "esc".to_string();
    let mut logging = true;
    let mut db: Option<PathBuf> = None;

    let mut it = ours.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--idle-ms" => idle_ms = parse_num(it.next(), "--idle-ms")?,
            "--tick-ms" => tick_ms = parse_num(it.next(), "--tick-ms")?,
            "--submit-newline" => submit_newline = true,
            "--interrupt" => {
                interrupt = it
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--interrupt requires a value"))?
                    .clone()
            }
            "--db" => {
                let p = it.next().ok_or_else(|| anyhow::anyhow!("--db requires a path"))?;
                db = Some(PathBuf::from(p));
            }
            "--no-log" => logging = false,
            other => anyhow::bail!("unknown option `{other}` (did you forget `--` before the agent command?)"),
        }
    }

    if agent_cmd.is_empty() {
        eprint!("{HELP}");
        anyhow::bail!("no agent command provided");
    }

    let settings = Settings {
        agent_command: agent_cmd,
        idle_after_ms: idle_ms,
        tick_ms,
        submit: if submit_newline { b"\n".to_vec() } else { b"\r".to_vec() },
        interrupt_bytes: interrupt_bytes(&interrupt),
        interrupt_label: interrupt,
        rows: 40,
        cols: 120,
    };

    let arbiter = Box::new(HeuristicArbiter::new(
        DEFAULT_INTERRUPT_KEYWORDS.iter().map(|s| s.to_string()).collect(),
    ));

    let memlog = if logging {
        let session_id = format!("delphin-{}-{}", Utc::now().format("%Y%m%dT%H%M%SZ"), std::process::id());
        let cwd = std::env::current_dir().ok().and_then(|p| p.to_str().map(str::to_string));
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
