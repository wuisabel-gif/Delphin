//! End-to-end test: drive the built `delphin` binary against the mock agent and
//! verify the conversation lands in the SQLite memory with the right verdicts.
//!
//! It exercises the real PTY + supervisor + arbiter + memory path that unit tests
//! can't reach. It is `#[ignore]`d by default because it depends on wall-clock
//! timing (the idle detector), which is fine locally but flaky on shared CI.
//!
//! Run it explicitly:
//!     cargo test --test e2e -- --ignored

use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[test]
#[ignore = "timing-dependent end-to-end test; run with --ignored"]
fn logs_turns_with_verdicts_to_db() {
    let bin = env!("CARGO_BIN_EXE_delphin");
    let mock = format!("{}/examples/mock-agent.sh", env!("CARGO_MANIFEST_DIR"));
    let db = std::env::temp_dir().join(format!("delphin-e2e-{}.sqlite3", std::process::id()));
    let _ = std::fs::remove_file(&db);

    let mut child = Command::new(bin)
        .args([
            "--db",
            db.to_str().unwrap(),
            "--interrupt",
            "ctrl-c",
            "--idle-ms",
            "600",
            "--tick-ms",
            "100",
            "--submit-newline",
            "--",
            "bash",
            &mock,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn delphin");

    {
        let mut stdin = child.stdin.take().expect("stdin");
        thread::sleep(Duration::from_millis(1300)); // let the mock settle to idle
        writeln!(stdin, "first question").unwrap(); // idle -> send_now
        stdin.flush().unwrap();
        thread::sleep(Duration::from_millis(1000));
        writeln!(stdin, "also add logging").unwrap(); // busy -> enqueue
        stdin.flush().unwrap();
        thread::sleep(Duration::from_millis(500));
        writeln!(stdin, "stop wrong thing").unwrap(); // busy -> interrupt
        stdin.flush().unwrap();
        thread::sleep(Duration::from_millis(5000));
        // stdin dropped here -> EOF -> delphin shuts the agent down and exits
    }
    thread::sleep(Duration::from_millis(500));
    let _ = child.kill();
    let _ = child.wait();

    let conn = rusqlite::Connection::open(&db).expect("open memory db");
    let total: i64 = conn
        .query_row("SELECT count(*) FROM agent_turns", [], |r| r.get(0))
        .expect("count agent_turns");
    assert!(total > 0, "expected some agent_turns, got {total}");

    let verdicts: Vec<String> = conn
        .prepare("SELECT DISTINCT verdict FROM agent_turns WHERE verdict IS NOT NULL")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .flatten()
        .collect();
    assert!(
        verdicts.iter().any(|v| v == "send_now"),
        "expected a send_now verdict, got {verdicts:?}"
    );

    let _ = std::fs::remove_file(&db);
}
