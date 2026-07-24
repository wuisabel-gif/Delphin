//! Failure-mode hardening (roadmap 0.4): the agent can disappear unexpectedly
//! — a crash, an OOM kill, a panic — while a prompt is still sitting in
//! delphin's queue, never released. Prove that:
//!   1. delphin exits promptly and reports failure when that happens, and
//!   2. the fact that it happened, and that a prompt was dropped, is written
//!      to memory — a flight recorder that silently stops when the agent
//!      crashes would defeat its own purpose.
//!
//! `--idle-ms 100000` makes the scenario deterministic: it guarantees the
//! natural idle-release path can never fire before the scripted crash, so the
//! queued prompt is *definitely* still queued (not raced) when the agent dies.

use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Wraps `agent_script` (which must print a "you> " ready marker, read one
/// line, then disappear one way or another), sends one prompt through and a
/// second that stays queued, and asserts delphin exits cleanly with the
/// crash — and the dropped prompt — recorded in memory. Shared by the exit(1)
/// and SIGKILL scenarios below; only how the agent disappears differs.
fn assert_survives_unexpected_exit(agent_script: &str, db_tag: &str) {
    let bin = env!("CARGO_BIN_EXE_delphin");
    let agent = format!("{}/examples/{agent_script}", env!("CARGO_MANIFEST_DIR"));
    let db = std::env::temp_dir().join(format!(
        "delphin-{db_tag}-conformance-{}.sqlite3",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&db);

    let mut child = Command::new(bin)
        .args([
            "--db",
            db.to_str().unwrap(),
            "--interrupt",
            "ctrl-c",
            "--ready",
            "you> ",
            "--idle-ms",
            "100000", // deliberately huge: no natural release can race the crash
            "--tick-ms",
            "100",
            "--submit-newline",
            "--",
            "bash",
            &agent,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn delphin");

    {
        let mut stdin = child.stdin.take().expect("stdin");
        thread::sleep(Duration::from_millis(1500)); // boot: ready marker -> idle
        writeln!(stdin, "first task").unwrap(); // idle -> send_now, agent goes busy
        stdin.flush().unwrap();
        thread::sleep(Duration::from_millis(200));
        writeln!(stdin, "also add tests").unwrap(); // busy -> enqueue, never released
        stdin.flush().unwrap();
        // The agent processes "first task" then disappears ~0.2-0.3s later;
        // wait for that real exit before dropping stdin ourselves, or
        // dropping stdin here would send our own EOF and hide the crash path.
        thread::sleep(Duration::from_millis(1000));
    }

    // delphin must exit on its own shortly after the agent does — no hang.
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "delphin did not exit after the wrapped agent disappeared"
        );
        thread::sleep(Duration::from_millis(50));
    };
    assert!(
        !status.success(),
        "delphin should propagate the wrapped process failure, got {status}"
    );

    let conn = rusqlite::Connection::open(&db).expect("open memory db");
    let verdicts: Vec<String> = conn
        .prepare("SELECT verdict FROM agent_turns WHERE verdict IS NOT NULL")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .flatten()
        .collect();
    assert!(verdicts.iter().any(|v| v == "send_now"));
    assert!(verdicts.iter().any(|v| v == "enqueue"));

    // The core assertion: the crash — and the dropped queued prompt — must be
    // in the record, not silently lost.
    let exit_note: Option<String> = conn
        .query_row(
            "SELECT text FROM agent_turns \
             WHERE direction = 'system' AND text LIKE 'agent exited%' \
             ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();
    let exit_note = exit_note.expect("an 'agent exited' system row must be logged");
    assert!(
        exit_note.contains("1 prompt"),
        "the exit note should name the one prompt that was still queued, got: {exit_note}"
    );

    let _ = std::fs::remove_file(&db);
}

#[test]
fn queued_prompt_survives_a_graceful_agent_exit() {
    assert_survives_unexpected_exit("crashy-agent.sh", "crash");
}

#[test]
fn queued_prompt_survives_an_abrupt_sigkill() {
    // "Link loss" (roadmap 0.4): the PTY reader's Ok(0) | Err(_) branches are
    // already merged into one AgentExited path, which is the right call
    // cross-platform (Linux PTYs often surface EIO, not a clean EOF, once
    // every slave fd is closed) — but that was never proven against an
    // abrupt SIGKILL specifically, only a graceful `exit 1`. This is that
    // proof: the wrapped process vanishes with no shutdown of its own at all.
    assert_survives_unexpected_exit("self-kill-agent.sh", "selfkill");
}
