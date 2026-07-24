#![cfg(unix)]

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[test]
fn passthrough_forwards_terminal_escape_sequences_byte_for_byte() {
    let bin = env!("CARGO_BIN_EXE_delphin");
    let mut child = Command::new(bin)
        .args([
            "--no-log",
            "--passthrough",
            "--",
            "bash",
            "-c",
            "stty raw -echo; dd bs=1 count=3 2>/dev/null | od -An -t u1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn delphin");

    let mut stdin = child.stdin.take().expect("stdin");
    thread::sleep(Duration::from_millis(200));
    stdin.write_all(b"\x1b[A").expect("write arrow-up bytes");
    stdin.flush().expect("flush input");

    let status = child.wait().expect("wait for delphin");
    assert!(status.success(), "delphin exited with {status}");

    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_string(&mut stdout)
        .expect("read output");
    assert!(
        stdout.contains("27  91  65") || stdout.contains("27 91 65"),
        "wrapped process did not receive arrow-up bytes: {stdout:?}"
    );
}
