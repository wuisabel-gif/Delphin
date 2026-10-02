//! Live "seen this before" hints from MemoryWhale.
//!
//! While the agent works, error-looking lines in its output are sent to
//! `mw hint`, one at a time on a background thread. When MemoryWhale has seen the
//! error before, a one-line hint is printed to stderr, for example:
//!
//! ```text
//! 🐬 MemoryWhale: seen 2 times, the fix was: xcode-select --install · type :fix to send it
//! ```
//!
//! The latest suggested fix is kept so the user can send it to the agent with
//! [`FIX_COMMAND`]. The fix comes from a local database and is untrusted text:
//! it is never executed, only quoted in a prompt, with control characters removed.

use std::collections::HashSet;
use std::process::{Command, Stdio};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
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
const MAX_FIX: usize = 500;
/// Typed at Delphin's prompt to send the latest suggested fix to the agent.
/// The `:` prefix keeps it clear of agent slash commands.
pub const FIX_COMMAND: &str = ":fix";

/// Splits agent output into lines and queues error-looking ones for lookup.
pub struct HintWatcher {
    partial: Vec<u8>,
    tx: SyncSender<String>,
    latest_fix: Arc<Mutex<Option<String>>>,
}

impl HintWatcher {
    /// Start the lookup thread. `mw` is the program to run (normally "mw").
    /// `offer_fix` appends the [`FIX_COMMAND`] hint; off in passthrough mode,
    /// where Delphin cannot read typed lines.
    pub fn spawn(mw: String, offer_fix: bool) -> Self {
        let latest_fix = Arc::new(Mutex::new(None));
        let shared = latest_fix.clone();
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
                    let hint = strip_controls(&hint);
                    let fix = parse_fix(&hint);
                    let action = if fix.is_some() && offer_fix {
                        format!(" · type {FIX_COMMAND} to send it")
                    } else {
                        String::new()
                    };
                    if fix.is_some() {
                        *shared.lock().unwrap() = fix;
                    }
                    eprint!("\r\n\x1b[2m🐬 MemoryWhale: {hint}{action}\x1b[0m\r\n");
                }
            }
        });
        Self {
            partial: Vec::new(),
            tx,
            latest_fix,
        }
    }

    /// The most recent fix MemoryWhale suggested, if any.
    pub fn latest_fix(&self) -> Option<String> {
        self.latest_fix.lock().unwrap().clone()
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

/// True when the user's line is the [`FIX_COMMAND`].
pub fn is_fix_command(line: &str) -> bool {
    line.trim() == FIX_COMMAND
}

/// The prompt that hands a suggested fix to the agent as text to evaluate.
pub fn fix_prompt(fix: &str) -> String {
    format!(
        "MemoryWhale says this error was fixed before by running `{fix}`. \
         Check whether that applies here."
    )
}

/// The command from `seen N times, the fix was: <command>`, if there is one.
fn parse_fix(hint: &str) -> Option<String> {
    let fix = hint.split_once("the fix was:")?.1.trim();
    (!fix.is_empty()).then(|| fix.chars().take(MAX_FIX).collect())
}

/// Drop control characters (escape sequences, newlines) from untrusted text,
/// so it can neither drive the terminal nor submit early to the agent.
fn strip_controls(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
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
            latest_fix: Arc::default(),
        };
        w.feed(b"ok line\nfatal: not a git rep");
        assert!(rx.try_recv().is_err(), "partial line waits for its end");
        w.feed(b"ository\r\n");
        assert_eq!(rx.try_recv().unwrap(), "fatal: not a git repository");
    }

    #[test]
    fn parses_fix_from_hint() {
        assert_eq!(
            parse_fix("seen 2 times, the fix was: xcode-select --install").as_deref(),
            Some("xcode-select --install")
        );
        assert_eq!(parse_fix("seen 3 times, no fix recorded yet"), None);
        assert_eq!(parse_fix("seen 1 times, the fix was:   "), None);
    }

    #[test]
    fn strips_control_characters() {
        assert_eq!(
            strip_controls("rm\x1b[2J -rf\r\nls\x07\tok"),
            "rm[2J -rflsok"
        );
    }

    #[test]
    fn builds_fix_prompt() {
        assert_eq!(
            fix_prompt("xcode-select --install"),
            "MemoryWhale says this error was fixed before by running \
             `xcode-select --install`. Check whether that applies here."
        );
    }

    #[test]
    fn recognizes_fix_command() {
        assert!(is_fix_command(":fix"));
        assert!(is_fix_command("  :fix "));
        assert!(!is_fix_command("/fix"));
        assert!(!is_fix_command(":fix it please"));
    }
}
