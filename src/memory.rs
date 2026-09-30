//! delphin's local memory — a tiny, self-contained SQLite log of the conversation.
//!
//! By default it writes to `<data_local>/Delphin/delphin.sqlite3`. Pass a custom path
//! (e.g. MemoryWhale's `memorywhale.sqlite3`) to let delphin *accompany* another
//! system's memory instead — companionship by choice, not dependency.
//!
//! With `--memorywhale`, turns go through `mw turns` instead: MemoryWhale owns
//! its schema and redacts secrets before anything is stored.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;

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
    sink: Sink,
    session_id: String,
    cwd: Option<String>,
}

enum Sink {
    Sqlite {
        conn: Connection,
        path: PathBuf,
    },
    /// A long-lived `mw turns` child fed one JSON line per turn.
    MemoryWhale {
        stdin: Mutex<Option<ChildStdin>>,
        child: Child,
    },
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
        create_private_db_file(&db_path)?;
        let conn = Connection::open(&db_path)
            .with_context(|| format!("opening database {}", db_path.display()))?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA busy_timeout = 3000;
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
            ",
        )?;
        // Additive migration for DBs created before turn_group_id existed; the
        // error when the column is already present is expected and ignored.
        let _ = conn.execute(
            "ALTER TABLE agent_turns ADD COLUMN turn_group_id INTEGER",
            [],
        );
        validate_agent_turns_schema(&conn)?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_agent_turns_session ON agent_turns(session_id)",
            [],
        )?;
        Ok(Self {
            sink: Sink::Sqlite {
                conn,
                path: db_path,
            },
            session_id: session_id.into(),
            cwd,
        })
    }

    /// Record through MemoryWhale: spawn `<mw> turns --session <id> [--cwd <dir>]`
    /// and stream turns to it. `mw` is the program to run (normally "mw").
    pub fn memorywhale(
        mw: &str,
        session_id: impl Into<String>,
        cwd: Option<String>,
    ) -> anyhow::Result<Self> {
        let session_id = session_id.into();
        let mut cmd = Command::new(mw);
        cmd.args(["turns", "--session", &session_id]);
        if let Some(dir) = &cwd {
            cmd.args(["--cwd", dir]);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .with_context(|| format!("starting `{mw} turns` (is MemoryWhale installed?)"))?;
        let stdin = child.stdin.take();
        Ok(Self {
            sink: Sink::MemoryWhale {
                stdin: Mutex::new(stdin),
                child,
            },
            session_id,
            cwd,
        })
    }

    #[allow(dead_code)] // public accessor; part of MemoryLog's surface
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Where turns go, for the startup banner.
    pub fn location(&self) -> String {
        match &self.sink {
            Sink::Sqlite { path, .. } => path.display().to_string(),
            Sink::MemoryWhale { .. } => "MemoryWhale, via mw turns".to_string(),
        }
    }

    fn log(&self, direction: TurnDirection, verdict: Option<&str>, text: &str, group: u64) {
        let res = match &self.sink {
            Sink::Sqlite { conn, .. } => conn
                .execute(
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
                )
                .map(drop)
                .map_err(anyhow::Error::from),
            Sink::MemoryWhale { stdin, .. } => {
                let line = serde_json::json!({
                    "direction": direction.as_str(),
                    "verdict": verdict,
                    "text": text,
                    "group": group,
                });
                let mut pipe = stdin.lock().unwrap();
                match pipe.as_mut().map(|p| writeln!(p, "{line}")) {
                    Some(Err(e)) => {
                        // `mw turns` is gone (often an older MemoryWhale without
                        // it). Report once and stop, not once per turn.
                        *pipe = None;
                        Err(anyhow::anyhow!(
                            "{e}; MemoryWhale stopped accepting turns (needs mw 0.15 or newer), \
                             recording is off for this session"
                        ))
                    }
                    _ => Ok(()),
                }
            }
        };
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

impl Drop for MemoryLog {
    /// Close the pipe and wait, so `mw turns` stores the last turns before exit.
    fn drop(&mut self) {
        if let Sink::MemoryWhale { stdin, child } = &mut self.sink {
            drop(stdin.get_mut().unwrap().take());
            let _ = child.wait();
        }
    }
}

/// Validate the shared `agent_turns` contract before a session starts. This
/// turns an incompatible external `--db` schema into one actionable startup
/// error instead of silently dropping every later memory write.
fn validate_agent_turns_schema(conn: &Connection) -> anyhow::Result<()> {
    const REQUIRED: &[&str] = &[
        "id",
        "session_id",
        "ts",
        "direction",
        "verdict",
        "text",
        "cwd",
        "turn_group_id",
    ];
    let mut stmt = conn.prepare("PRAGMA table_info(agent_turns)")?;
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get(1))?
        .collect::<Result<_, _>>()?;
    let missing: Vec<&str> = REQUIRED
        .iter()
        .copied()
        .filter(|required| !columns.iter().any(|column| column == required))
        .collect();
    if !missing.is_empty() {
        anyhow::bail!(
            "incompatible agent_turns schema; missing column(s): {}",
            missing.join(", ")
        );
    }
    Ok(())
}

/// Pre-create a new database with owner-only permissions on Unix. Existing
/// databases are left untouched because `--db` may point at a shared database
/// whose access policy Delphin does not own.
#[cfg(unix)]
fn create_private_db_file(path: &std::path::Path) -> anyhow::Result<()> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;

    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e).with_context(|| format!("creating database {}", path.display())),
    }
}

#[cfg(not(unix))]
fn create_private_db_file(_path: &std::path::Path) -> anyhow::Result<()> {
    Ok(())
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
        let escaped = q
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        conn.prepare(
            "SELECT session_id, ts, direction, verdict, text, turn_group_id \
             FROM agent_turns WHERE text LIKE ?1 ESCAPE '\\' ORDER BY id DESC LIMIT ?2",
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

    #[cfg(unix)]
    #[test]
    fn memorywhale_sink_streams_json_lines_to_mw_turns() {
        let dir = std::env::temp_dir().join(format!("delphin-mw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A stand-in `mw` that records its arguments and stdin.
        let fake = dir.join("mw");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\necho \"$@\" > {0}/args\ncat > {0}/turns\n",
                dir.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

        let ml = MemoryLog::memorywhale(fake.to_str().unwrap(), "s9", Some("/w".into())).unwrap();
        ml.user("add tests", "enqueue", 2);
        ml.agent("\x1b[1mdone\x1b[0m", 2);
        drop(ml); // waits for the child

        let args = std::fs::read_to_string(dir.join("args")).unwrap();
        assert_eq!(args.trim(), "turns --session s9 --cwd /w");
        let turns: Vec<serde_json::Value> = std::fs::read_to_string(dir.join("turns"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0]["direction"], "user");
        assert_eq!(turns[0]["verdict"], "enqueue");
        assert_eq!(turns[0]["group"], 2);
        assert_eq!(turns[1]["text"], "done");
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

    #[test]
    fn search_treats_like_metacharacters_as_literals() {
        let dir =
            std::env::temp_dir().join(format!("delphin-search-literal-{}", std::process::id()));
        let db = dir.join("delphin.sqlite3");
        let _ = std::fs::remove_dir_all(&dir);
        let ml = MemoryLog::open("s1", None, Some(db.clone())).unwrap();
        ml.user("progress is 100% complete", "send_now", 1);
        ml.user("progress is 100 percent complete", "send_now", 2);
        ml.user("use snake_case here", "send_now", 3);
        ml.user("use snakeXcase here", "send_now", 4);
        ml.user(r"open path\name", "send_now", 5);
        ml.user("open pathname", "send_now", 6);
        drop(ml);

        for (query, expected) in [
            ("100%", "progress is 100% complete"),
            ("snake_case", "use snake_case here"),
            (r"path\name", r"open path\name"),
        ] {
            let hits = search(Some(db.clone()), query, 10).unwrap();
            assert_eq!(hits.len(), 1, "query {query:?} must match literally");
            assert_eq!(hits[0].text, expected);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn memorywhale_agent_turns_fixture_is_compatible() {
        let dir = std::env::temp_dir().join(format!("delphin-memorywhale-{}", std::process::id()));
        let db = dir.join("memorywhale.sqlite3");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // MemoryWhale's shared contract intentionally omits Delphin's optional
        // grouping column; Delphin adds it through its existing migration.
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE agent_turns (
                id INTEGER PRIMARY KEY,
                session_id TEXT NOT NULL,
                ts TEXT NOT NULL,
                direction TEXT NOT NULL,
                verdict TEXT,
                text TEXT NOT NULL,
                cwd TEXT
            );",
        )
        .unwrap();
        drop(conn);

        let ml = MemoryLog::open("shared-session", None, Some(db.clone())).unwrap();
        ml.user("shared memory", "send_now", 4);
        drop(ml);

        // This is the projection MemoryWhale uses for retrieval.
        let conn = Connection::open(&db).unwrap();
        let row: (i64, String, String, String) = conn
            .query_row(
                "SELECT id, ts, direction, text FROM agent_turns",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(row.2, "user");
        assert_eq!(row.3, "shared memory");

        let has_group_column: bool = conn
            .prepare("PRAGMA table_info(agent_turns)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .flatten()
            .any(|column| column == "turn_group_id");
        assert!(has_group_column);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn incompatible_agent_turns_schema_is_rejected_at_open() {
        let dir = std::env::temp_dir().join(format!("delphin-incompatible-{}", std::process::id()));
        let db = dir.join("external.sqlite3");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE agent_turns (id INTEGER PRIMARY KEY, text TEXT NOT NULL);",
        )
        .unwrap();
        drop(conn);

        let error = match MemoryLog::open("s1", None, Some(db)) {
            Ok(_) => panic!("incompatible schema should fail"),
            Err(error) => error,
        };
        let message = error.to_string();
        assert!(message.contains("incompatible agent_turns schema"));
        assert!(message.contains("session_id"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn newly_created_database_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("delphin-permissions-{}", std::process::id()));
        let db = dir.join("delphin.sqlite3");
        let _ = std::fs::remove_dir_all(&dir);

        let ml = MemoryLog::open("s1", None, Some(db.clone())).unwrap();
        drop(ml);

        let mode = std::fs::metadata(&db).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
