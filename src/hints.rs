//! Live "seen this before" hints from MemoryWhale.
//!
//! While the agent works, error-looking lines in its output are sent to
//! `mw hint`, one at a time on a background thread. When MemoryWhale has seen the
//! error before, a one-line hint is printed to stderr, for example:
//!
//! ```text
//! 🐬 MemoryWhale: seen 2 times, the fix was: xcode-select --install
//! ```

use std::collections::HashSet;
use std::process::{Command, Stdio};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::thread;

use crate::memory::strip_ansi;

const KEYWORDS: &[&str] = &[
    "error",
    "failed",
    "fatal",
    "panic",
    "exception",
    "not found",
    "no such file",
    "cannot",
];
const MAX_PARTIAL: usize = 4096;
const MAX_LINE: usize = 300;

/// Splits agent output into lines and queues error-looking ones for lookup.
pub struct HintWatcher {
    partial: Vec<u8>,
    tx: SyncSender<String>,
}

impl HintWatcher {
    /// Start the lookup thread. `mw` is the program to run (normally "mw").
    pub fn spawn(mw: String) -> Self {
        // ponytail: a small bounded queue; lines past it are dropped, since a
        // hint that arrives late is noise.
        let (tx, rx) = sync_channel::<String>(8);
        thread::spawn(move || {
            let mut asked = HashSet::new();
            for line in rx {
                if !asked.insert(line.to_lowercase()) {
                    continue;
                }
                if let Some(hint) = ask(&mw, &line) {
                    eprint!("\r\n\x1b[2m🐬 MemoryWhale: {hint}\x1b[0m\r\n");
                }
            }
        });
        Self {
            partial: Vec::new(),
            tx,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.partial.extend_from_slice(bytes);
        while let Some(end) = self.partial.iter().position(|b| *b == b'\n' || *b == b'\r') {
            let raw: Vec<u8> = self.partial.drain(..=end).collect();
            if let Some(line) = error_line(&String::from_utf8_lossy(&raw)) {
                let _ = self.tx.try_send(line);
            }
        }
        if self.partial.len() > MAX_PARTIAL {
            self.partial.clear();
        }
    }
}

/// The cleaned line if it looks like an error worth asking about.
fn error_line(raw: &str) -> Option<String> {
    let line = strip_ansi(raw).trim().to_string();
    let lower = line.to_lowercase();
    if line.len() < 12 || !KEYWORDS.iter().any(|k| lower.contains(k)) {
        return None;
    }
    Some(line.chars().take(MAX_LINE).collect())
}

fn ask(mw: &str, line: &str) -> Option<String> {
    let out = Command::new(mw)
        .args(["hint", "--", line])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let hint = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !hint.is_empty()).then_some(hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_error_lines_only() {
        assert_eq!(
            error_line("\x1b[31m  ⎿  Error: xcrun: error: invalid path\x1b[0m\r\n").as_deref(),
            Some("⎿  Error: xcrun: error: invalid path")
        );
        assert_eq!(error_line("Compiling delphin v0.4.0\n"), None);
        assert_eq!(error_line("error\n"), None, "too short to be useful");
    }

    #[test]
    fn feed_splits_lines_across_chunks() {
        let (tx, rx) = sync_channel(8);
        let mut w = HintWatcher {
            partial: Vec::new(),
            tx,
        };
        w.feed(b"ok line\nfatal: not a git rep");
        assert!(rx.try_recv().is_err(), "partial line waits for its end");
        w.feed(b"ository\r\n");
        assert_eq!(rx.try_recv().unwrap(), "fatal: not a git repository");
    }
}
