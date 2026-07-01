use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::Connection;

#[test]
fn live_mode_records_busy_prompt_as_stream() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let db_path =
        std::env::temp_dir().join(format!("delphin-live-mode-{}.sqlite3", std::process::id()));
    let _ = std::fs::remove_file(&db_path);

    let mut child = Command::new(env!("CARGO_BIN_EXE_delphin"))
        .current_dir(manifest_dir)
        .args([
            "--live",
            "--idle-ms",
            "100",
            "--tick-ms",
            "25",
            "--interrupt",
            "ctrl-c",
            "--db",
            db_path.to_str().unwrap(),
            "--",
            "bash",
            "examples/mock-agent.sh",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    thread::sleep(Duration::from_millis(300));
    writeln!(stdin, "first").unwrap();
    thread::sleep(Duration::from_millis(300));
    writeln!(stdin, "second").unwrap();
    drop(stdin);

    wait_or_kill(&mut child, Duration::from_secs(3));

    let conn = Connection::open(&db_path).unwrap();
    let verdict: String = conn
        .query_row(
            "SELECT verdict FROM agent_turns WHERE direction = 'user' AND text = 'second'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(verdict, "stream");

    let _ = std::fs::remove_file(&db_path);
}

fn wait_or_kill(child: &mut Child, timeout: Duration) {
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            return;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
}
