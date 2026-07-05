//! Multi-agent conformance: prove the supervisor/arbiter loop holds up
//! against a PTY agent with a meaningfully different shape than
//! examples/mock-agent.sh — one that produces ZERO output while working (no
//! dots, no streaming) and never prints a ready marker. This is the
//! "arbitrary PTY CLI" case roadmap item 0.3 asks for, and the scenario
//! roadmap item 0.1 names as a known risk ("tool-call pauses that look idle
//! but aren't").
//!
//! `--min-busy-ms` is tuned to this agent's known ~3s work time (4s) — the fix
//! the golden-transcript tests in src/supervisor.rs document for the gap that
//! shows up at default settings. This test proves that fix holds through the
//! real PTY + supervisor loop, not just the pure `is_idle_now` function.

use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[test]
fn silent_agent_queues_and_releases_correctly_when_min_busy_is_tuned() {
    let bin = env!("CARGO_BIN_EXE_delphin");
    let agent = format!("{}/examples/silent-agent.sh", env!("CARGO_MANIFEST_DIR"));
    let db = std::env::temp_dir().join(format!(
        "delphin-silent-conformance-{}.sqlite3",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&db);

    let mut child = Command::new(bin)
        .args([
            "--db",
            db.to_str().unwrap(),
            "--interrupt",
            "ctrl-c",
            "--min-busy-ms",
            "4000",
            "--idle-ms",
            "800",
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
        // Clear the boot min-busy floor before typing, so the first prompt
        // lands while genuinely idle (not still "assumed busy" from startup).
        // The 4000ms floor is exact (proven by the pure-function golden-
        // transcript tests); this margin absorbs real process/thread slop.
        thread::sleep(Duration::from_millis(6500));
        writeln!(stdin, "add a login endpoint").unwrap(); // idle -> send_now
        stdin.flush().unwrap();
        thread::sleep(Duration::from_millis(500)); // well within the ~3s silent work window
        writeln!(stdin, "also add tests").unwrap(); // busy, floor not yet elapsed -> enqueue
        stdin.flush().unwrap();
        // min_busy_ms (4s) exceeds the agent's real work time (~3s), so the
        // floor — not the work finishing — is what release actually waits on.
        thread::sleep(Duration::from_millis(7000));
        // stdin dropped -> EOF -> delphin shuts the agent down and exits
    }
    thread::sleep(Duration::from_millis(500));
    let _ = child.kill();
    let _ = child.wait();

    let conn = rusqlite::Connection::open(&db).expect("open memory db");
    let verdicts: Vec<String> = conn
        .prepare("SELECT verdict FROM agent_turns WHERE verdict IS NOT NULL")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .flatten()
        .collect();
    for want in ["send_now", "enqueue"] {
        assert!(
            verdicts.iter().any(|v| v == want),
            "expected a {want} verdict against the silent agent; got {verdicts:?}"
        );
    }

    let released: i64 = conn
        .query_row(
            "SELECT count(*) FROM agent_turns WHERE direction = 'system' AND text LIKE 'released #%'",
            [],
            |r| r.get(0),
        )
        .expect("count released rows");
    assert!(
        released > 0,
        "the queued prompt should be released once the tuned min-busy floor elapses"
    );

    let _ = std::fs::remove_file(&db);
}
