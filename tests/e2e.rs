//! End-to-end test: drive the built `delphin` binary against the mock agent and
//! verify the whole PTY + supervisor + arbiter + memory loop — the part unit
//! tests can't reach. It asserts the three behaviours delphin exists for:
//!   * a prompt sent while idle is **send_now**,
//!   * an ordinary prompt sent while busy is **enqueued**,
//!   * an urgency prompt sent while busy **interrupts**,
//!   * and the enqueued prompt is later **released** when the agent goes idle.
//!
//! `--ready 'you> '` keys idle detection off the mock's prompt instead of the
//! silence timer, so the run is deterministic enough to keep on by default.
//!
//! ponytail: verification reads the SQLite memory the run already writes (verdicts
//! + a "released #N" system row), so no new observation channel is needed. Sleeps
//! are generous; if a loaded CI still makes it flaky, re-add `#[ignore]`.

use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[test]
fn queue_interrupt_release_end_to_end() {
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
            "--ready",
            "you> ",
            "--idle-ms",
            "500",
            "--tick-ms",
            "80",
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
        thread::sleep(Duration::from_millis(2000)); // boot: mock shows "you> " -> idle
        writeln!(stdin, "first question").unwrap(); // idle -> send_now, mock goes busy
        stdin.flush().unwrap();
        thread::sleep(Duration::from_millis(800));
        writeln!(stdin, "also add logging").unwrap(); // busy -> enqueue (#1)
        stdin.flush().unwrap();
        // Let the first prompt finish on its own (~4s think) so the queue drains
        // deterministically — the release assertion doesn't hang on interrupt timing.
        thread::sleep(Duration::from_millis(4800));
        writeln!(stdin, "stop wrong thing").unwrap(); // agent busy on #1 -> interrupt
        stdin.flush().unwrap();
        thread::sleep(Duration::from_millis(1500));
        // stdin dropped here -> EOF -> delphin shuts the agent down and exits
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
    for want in ["send_now", "enqueue", "interrupt"] {
        assert!(
            verdicts.iter().any(|v| v == want),
            "expected a {want} verdict; got {verdicts:?}"
        );
    }

    // The enqueued prompt must actually have been released (supervisor logs a
    // "released #N: ..." system row when it drains the queue on idle).
    let released: i64 = conn
        .query_row(
            "SELECT count(*) FROM agent_turns WHERE direction = 'system' AND text LIKE 'released #%'",
            [],
            |r| r.get(0),
        )
        .expect("count released rows");
    assert!(
        released > 0,
        "expected the queued prompt to be released, but no 'released #N' row was logged"
    );

    // Causal link: the enqueued prompt and its release row share a turn_group_id.
    let enqueued_group: Option<i64> = conn
        .query_row(
            "SELECT turn_group_id FROM agent_turns WHERE direction = 'user' AND text = 'also add logging'",
            [],
            |r| r.get(0),
        )
        .expect("find enqueued prompt row");
    let released_group: Option<i64> = conn
        .query_row(
            "SELECT turn_group_id FROM agent_turns WHERE direction = 'system' AND text LIKE 'released #%'",
            [],
            |r| r.get(0),
        )
        .expect("find release row");
    assert!(enqueued_group.is_some(), "enqueued prompt should have a group");
    assert_eq!(
        enqueued_group, released_group,
        "the queued prompt and its release must share a turn_group_id"
    );

    let _ = std::fs::remove_file(&db);
}
