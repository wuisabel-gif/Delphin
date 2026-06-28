//! delphin's local memory — a tiny, self-contained SQLite log of the conversation.
//!
//! By default it writes to `<data_local>/Delphin/delphin.sqlite3`. Pass a custom path
//! (e.g. MemoryWhale's `memorywhale.sqlite3`) to let delphin *accompany* another
//! system's memory instead — companionship by choice, not dependency.

use std::path::PathBuf;

use anyhow::Context;
use chrono::Utc;
use rusqlite::{params, Connection};

/// Default database path: `<platform-data-local>/Delphin/delphin.sqlite3`.
pub fn default_db_path() -> anyhow::Result<PathBuf> {
    let base = dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| anyhow::anyhow!("could not resolve a local data directory"))?;
    Ok(base.join("Delphin").join("delphin.sqlite3"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnDirection {
    User,
    Agent,
    System,
}

impl TurnDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            TurnDirection::User => "user",
            TurnDirection::Agent => "agent",
            TurnDirection::System => "system",
        }
    }
}

/// Owns its connection so callers just `.user()/.agent()/.system()`.
/// Logging failures are reported to stderr but never propagated — remembering
/// must not crash the conversation.
pub struct MemoryLog {
    conn: Connection,
    session_id: String,
    cwd: Option<String>,
    db_path: PathBuf,
}

impl MemoryLog {
    /// Open (or create) the memory database and ensure the schema.
    /// `db_path` overrides the default location when provided.
    pub fn open(
        session_id: impl Into<String>,
        cwd: Option<String>,
        db_path: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let db_path = match db_path {
            Some(p) => p,
            None => default_db_path()?,
        };
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating data dir {}", parent.display()))?;
        }
        let conn = Connection::open(&db_path)
            .with_context(|| format!("opening database {}", db_path.display()))?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS agent_turns (
                id INTEGER PRIMARY KEY,
                session_id TEXT NOT NULL,
                ts TEXT NOT NULL,
                direction TEXT NOT NULL,   -- 'user' | 'agent' | 'system'
                verdict TEXT,              -- 'send_now' | 'enqueue' | 'interrupt' | NULL
                text TEXT NOT NULL,
                cwd TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_agent_turns_session ON agent_turns(session_id);
            ",
        )?;
        Ok(Self {
            conn,
            session_id: session_id.into(),
            cwd,
            db_path,
        })
    }

    #[allow(dead_code)] // public accessor; part of MemoryLog's surface
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }

    fn log(&self, direction: TurnDirection, verdict: Option<&str>, text: &str) {
        let res = self.conn.execute(
            "INSERT INTO agent_turns (session_id, ts, direction, verdict, text, cwd)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                self.session_id,
                Utc::now().to_rfc3339(),
                direction.as_str(),
                verdict,
                text,
                self.cwd,
            ],
        );
        if let Err(e) = res {
            eprintln!("[delphin] memory log failed: {e}");
        }
    }

    /// Log a user prompt with the arbiter's verdict.
    pub fn user(&self, text: &str, verdict: &str) {
        self.log(TurnDirection::User, Some(verdict), text);
    }

    /// Log an agent reply (ANSI-stripped before storage).
    pub fn agent(&self, text: &str) {
        self.log(TurnDirection::Agent, None, &strip_ansi(text));
    }

    /// Log a supervisor event (e.g. a queued prompt being released).
    pub fn system(&self, text: &str) {
        self.log(TurnDirection::System, None, text);
    }
}

/// Strip ANSI / terminal control sequences so stored agent output is readable.
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                // ESC: skip an escape sequence.
                match chars.peek() {
                    Some('[') => {
                        chars.next();
                        // CSI: params/intermediates until a final byte @-~.
                        while let Some(&n) = chars.peek() {
                            chars.next();
                            if ('@'..='~').contains(&n) {
                                break;
                            }
                        }
                    }
                    Some(']') => {
                        chars.next();
                        // OSC: until BEL or ESC\.
                        while let Some(&n) = chars.peek() {
                            if n == '\x07' {
                                chars.next();
                                break;
                            }
                            if n == '\x1b' {
                                chars.next();
                                if chars.peek() == Some(&'\\') {
                                    chars.next();
                                }
                                break;
                            }
                            chars.next();
                        }
                    }
                    Some(_) => {
                        chars.next();
                    }
                    None => {}
                }
            }
            '\r' => {}
            // Drop other C0 control chars except tab/newline.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' => {}
            '\x7f' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_and_log_roundtrip() {
        // in-memory DB via explicit path is awkward; use a temp file.
        let dir = std::env::temp_dir().join(format!("delphin-test-{}", std::process::id()));
        let db = dir.join("delphin.sqlite3");
        let ml = MemoryLog::open("s1", Some("/tmp".into()), Some(db.clone())).unwrap();
        ml.user("also add logging", "enqueue");
        ml.agent("\x1b[31mhello\x1b[0m");
        let conn = Connection::open(&db).unwrap();
        let (dir_s, verdict, text): (String, Option<String>, String) = conn
            .query_row(
                "SELECT direction, verdict, text FROM agent_turns WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(dir_s, "user");
        assert_eq!(verdict.as_deref(), Some("enqueue"));
        assert_eq!(text, "also add logging");
        let agent_text: String = conn
            .query_row("SELECT text FROM agent_turns WHERE id = 2", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(agent_text, "hello");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strip_ansi_removes_escapes() {
        assert_eq!(strip_ansi("\x1b[31mhi\x1b[0m\r\nthere"), "hi\nthere");
    }
}
