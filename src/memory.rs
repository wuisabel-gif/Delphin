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
                verdict TEXT,              -- 'send_now' | 'enqueue' | 'interrupt' | 'stream' | NULL
                text TEXT NOT NULL,
                cwd TEXT,
                turn_group_id INTEGER      -- a prompt, its release, and the reply it triggered share this
            );
            CREATE INDEX IF NOT EXISTS idx_agent_turns_session ON agent_turns(session_id);
            ",
        )?;
        // Additive migration for DBs created before turn_group_id existed; the
        // error when the column is already present is expected and ignored.
        let _ = conn.execute(
            "ALTER TABLE agent_turns ADD COLUMN turn_group_id INTEGER",
            [],
        );
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

    fn log(&self, direction: TurnDirection, verdict: Option<&str>, text: &str, group: u64) {
        let res = self.conn.execute(
            "INSERT INTO agent_turns (session_id, ts, direction, verdict, text, cwd, turn_group_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                self.session_id,
                Utc::now().to_rfc3339(),
                direction.as_str(),
                verdict,
                text,
                self.cwd,
                group as i64,
            ],
        );
        if let Err(e) = res {
            eprintln!("[delphin] memory log failed: {e}");
        }
    }

    /// Log a user prompt with the arbiter's verdict. `group` links it to the agent
    /// reply it triggers (and, for a queued prompt, to its later release row).
    pub fn user(&self, text: &str, verdict: &str, group: u64) {
        self.log(TurnDirection::User, Some(verdict), text, group);
    }

    /// Log an agent reply (ANSI-stripped before storage), stamped with the group
    /// of the prompt the agent is answering.
    pub fn agent(&self, text: &str, group: u64) {
        self.log(TurnDirection::Agent, None, &strip_ansi(text), group);
    }

    /// Log a supervisor event (e.g. a queued prompt being released), stamped with
    /// the group of the prompt it concerns.
    pub fn system(&self, text: &str, group: u64) {
        self.log(TurnDirection::System, None, text, group);
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

/// One row returned by [`search`].
#[derive(Debug, Clone)]
pub struct RecalledTurn {
    pub session_id: String,
    pub ts: String,
    pub direction: String,
    pub verdict: Option<String>,
    pub text: String,
    pub turn_group_id: Option<i64>,
}

/// Search the conversation memory for turns whose text matches `query`
/// (case-insensitive substring). An empty query returns the most recent turns.
/// Newest first, capped at `limit`. Read-only; returns an empty Vec if the
/// database doesn't exist yet.
pub fn search(
    db_path: Option<PathBuf>,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<RecalledTurn>> {
    let path = match db_path {
        Some(p) => p,
        None => default_db_path()?,
    };
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn =
        Connection::open(&path).with_context(|| format!("opening database {}", path.display()))?;
    let map = |r: &rusqlite::Row<'_>| {
        Ok(RecalledTurn {
            session_id: r.get(0)?,
            ts: r.get(1)?,
            direction: r.get(2)?,
            verdict: r.get(3)?,
            text: r.get(4)?,
            turn_group_id: r.get(5)?,
        })
    };
    let q = query.trim();
    let rows = if q.is_empty() {
        conn.prepare(
            "SELECT session_id, ts, direction, verdict, text, turn_group_id \
             FROM agent_turns ORDER BY id DESC LIMIT ?1",
        )?
        .query_map(params![limit as i64], map)?
        .collect::<Result<Vec<_>, _>>()?
    } else {
        let pattern = format!("%{q}%");
        conn.prepare(
            "SELECT session_id, ts, direction, verdict, text, turn_group_id \
             FROM agent_turns WHERE text LIKE ?1 ORDER BY id DESC LIMIT ?2",
        )?
        .query_map(params![pattern, limit as i64], map)?
        .collect::<Result<Vec<_>, _>>()?
    };
    Ok(rows)
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
        ml.user("also add logging", "enqueue", 7);
        ml.agent("\x1b[31mhello\x1b[0m", 7);
        let conn = Connection::open(&db).unwrap();
        let (dir_s, verdict, text, group): (String, Option<String>, String, Option<i64>) = conn
            .query_row(
                "SELECT direction, verdict, text, turn_group_id FROM agent_turns WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(dir_s, "user");
        assert_eq!(verdict.as_deref(), Some("enqueue"));
        assert_eq!(text, "also add logging");
        assert_eq!(group, Some(7), "user prompt and its reply share the group");
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

    #[test]
    fn search_finds_matching_turns() {
        let dir = std::env::temp_dir().join(format!("delphin-search-{}", std::process::id()));
        let db = dir.join("delphin.sqlite3");
        let _ = std::fs::remove_dir_all(&dir);
        let ml = MemoryLog::open("s1", None, Some(db.clone())).unwrap();
        ml.user("use Postgres for storage", "enqueue", 1);
        ml.user("add a logging flag", "send_now", 2);
        ml.agent("I'll set up Postgres now", 1);

        let hits = search(Some(db.clone()), "postgres", 10).unwrap();
        assert_eq!(hits.len(), 2, "should match both Postgres turns");
        assert!(hits
            .iter()
            .all(|h| h.text.to_lowercase().contains("postgres")));

        // empty query -> most recent turns (all 3), newest first
        let recent = search(Some(db.clone()), "", 10).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].direction, "agent");

        // missing DB -> empty, no error
        let none = search(Some(dir.join("nope.sqlite3")), "x", 5).unwrap();
        assert!(none.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
