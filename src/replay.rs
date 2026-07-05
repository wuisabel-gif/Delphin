//! Replay real conversation history through any [`crate::arbiter::Arbiter`].
//!
//! `delphin replay` walks a memory database's `agent_turns`, reconstructs the
//! `Decision` delphin would have built for each user prompt (phase from the
//! verdict that was actually recorded, busy-elapsed time and queue depth by
//! simulating the same enqueue/release events), and re-runs a chosen arbiter
//! over that history — comparing what it would decide against what actually
//! happened. This is Phase 1's replay harness: how a new or changed policy
//! gets checked against real usage before it ever runs live.
//!
//! ponytail: reconstruction uses only what's already logged, no new columns.
//! One gap: if a session ends with prompts still queued, there's no "went
//! idle" event to close them out, so a later turn's apparent busy time is
//! measured against the last known busy transition. Fine for comparing
//! arbiters; not a claim of perfect historical fidelity.

use std::path::PathBuf;

use anyhow::Context;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use crate::arbiter::{AgentPhase, Arbiter, Decision};

/// One replayed user turn: what actually happened vs. what `arbiter` decides
/// given the same reconstructed context.
#[derive(Debug, Clone)]
pub struct ReplayedTurn {
    pub session_id: String,
    pub ts: String,
    pub text: String,
    pub recorded_verdict: String,
    pub replayed_verdict: &'static str,
}

impl ReplayedTurn {
    pub fn agrees(&self) -> bool {
        self.recorded_verdict == self.replayed_verdict
    }
}

/// Replay every user turn in `db_path` (or the default database) through
/// `arbiter`, optionally restricted to one `session_id`. Read-only; returns an
/// empty Vec if the database doesn't exist yet.
pub fn replay(
    db_path: Option<PathBuf>,
    session_filter: Option<&str>,
    arbiter: &dyn Arbiter,
) -> anyhow::Result<Vec<ReplayedTurn>> {
    let path = match db_path {
        Some(p) => p,
        None => crate::memory::default_db_path()?,
    };
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn =
        Connection::open(&path).with_context(|| format!("opening database {}", path.display()))?;

    let rows: Vec<(String, String, String, Option<String>, String)> = conn
        .prepare(
            "SELECT session_id, ts, direction, verdict, text \
             FROM agent_turns WHERE (?1 IS NULL OR session_id = ?1) \
             ORDER BY session_id, id",
        )?
        .query_map(params![session_filter], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut out = Vec::new();
    let mut current_session: Option<String> = None;
    let mut busy_since: Option<DateTime<Utc>> = None;
    let mut queue_len: usize = 0;

    for (session_id, ts, direction, verdict, text) in rows {
        if current_session.as_deref() != Some(session_id.as_str()) {
            // Simulated state doesn't carry across sessions.
            current_session = Some(session_id.clone());
            busy_since = None;
            queue_len = 0;
        }
        let parsed_ts = DateTime::parse_from_rfc3339(&ts)
            .ok()
            .map(|t| t.with_timezone(&Utc));

        if direction == "system" && text.starts_with("released #") {
            queue_len = queue_len.saturating_sub(1);
            busy_since = parsed_ts;
            continue;
        }
        if direction != "user" {
            continue;
        }
        let Some(recorded_verdict) = verdict else {
            continue;
        };

        let phase = if recorded_verdict == "send_now" {
            AgentPhase::Idle
        } else {
            AgentPhase::Busy
        };
        let busy_elapsed_ms = match (phase, busy_since, parsed_ts) {
            (AgentPhase::Busy, Some(since), Some(now)) => {
                now.signed_duration_since(since).num_milliseconds().max(0) as u128
            }
            _ => 0,
        };

        let decision = Decision {
            phase,
            text: text.clone(),
            busy_elapsed_ms,
            queue_len,
        };
        let replayed_verdict = arbiter.decide(&decision).as_str();

        out.push(ReplayedTurn {
            session_id: session_id.clone(),
            ts: ts.clone(),
            text,
            recorded_verdict: recorded_verdict.clone(),
            replayed_verdict,
        });

        // Advance simulated state using the ORIGINAL verdict (what actually
        // happened), not the replayed one — we're comparing against reality.
        match recorded_verdict.as_str() {
            "send_now" | "interrupt" => busy_since = parsed_ts,
            "enqueue" => queue_len += 1,
            _ => {} // "stream": stays busy, doesn't touch the queue
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbiter::{HeuristicArbiter, QuestionArbiter, DEFAULT_INTERRUPT_KEYWORDS};
    use crate::memory::MemoryLog;

    fn keywords() -> Vec<String> {
        DEFAULT_INTERRUPT_KEYWORDS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn reconstructs_phase_and_queue_depth_from_history() {
        let dir = std::env::temp_dir().join(format!("delphin-replay-{}", std::process::id()));
        let db = dir.join("delphin.sqlite3");
        let _ = std::fs::remove_dir_all(&dir);
        let ml = MemoryLog::open("s1", None, Some(db.clone())).unwrap();

        ml.user("first task", "send_now", 1); // idle -> sent immediately
        ml.user("also add tests", "enqueue", 2); // busy -> queued
        ml.user("stop wrong thing", "interrupt", 3); // busy -> interrupted
        ml.system("released #1: also add tests", 2); // queue drains

        let arbiter = HeuristicArbiter::with_defaults();
        let turns = replay(Some(db.clone()), None, &arbiter).unwrap();

        assert_eq!(turns.len(), 3, "one entry per user turn");
        assert_eq!(turns[0].recorded_verdict, "send_now");
        assert_eq!(
            turns[0].replayed_verdict, "send_now",
            "idle always -> send_now, for every arbiter"
        );
        assert_eq!(turns[1].recorded_verdict, "enqueue");
        assert_eq!(turns[1].replayed_verdict, "enqueue");
        assert_eq!(turns[2].recorded_verdict, "interrupt");
        assert_eq!(turns[2].replayed_verdict, "interrupt");
        assert!(turns.iter().all(|t| t.agrees()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_database_replays_to_empty() {
        let arbiter = HeuristicArbiter::with_defaults();
        let missing = std::env::temp_dir().join("delphin-replay-does-not-exist.sqlite3");
        let turns = replay(Some(missing), None, &arbiter).unwrap();
        assert!(turns.is_empty());
    }

    #[test]
    fn a_different_arbiter_can_disagree_with_recorded_history() {
        // History was recorded under the heuristic (a plain question queues);
        // replaying with the question arbiter should flag a disagreement.
        let dir = std::env::temp_dir().join(format!("delphin-replay-diff-{}", std::process::id()));
        let db = dir.join("delphin.sqlite3");
        let _ = std::fs::remove_dir_all(&dir);
        let ml = MemoryLog::open("s1", None, Some(db.clone())).unwrap();
        ml.user("first task", "send_now", 1);
        ml.user("what database are we using?", "enqueue", 2);

        let question_arbiter = QuestionArbiter::new(keywords());
        let turns = replay(Some(db.clone()), None, &question_arbiter).unwrap();

        assert_eq!(turns[1].recorded_verdict, "enqueue");
        assert_eq!(
            turns[1].replayed_verdict, "interrupt",
            "the question arbiter should have interrupted for a question"
        );
        assert!(!turns[1].agrees());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
