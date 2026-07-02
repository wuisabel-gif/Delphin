//! The arbiter decides what to do with a user prompt that arrives while the
//! agent is mid-flight — the heart of delphin. It is a trait so the policy is
//! swappable; v1 ships the deterministic [`HeuristicArbiter`].
//!
//! Default policy: **the in-flight thinking is protected.** While the agent is
//! busy a new prompt is *queued*, UNLESS the user signals urgency with an
//! interrupt word ("stop", "wait", "cancel", "halt", …), in which case delphin
//! barges in. In live mode, ordinary busy prompts are streamed through instead
//! of queued. When the agent is idle, everything is sent immediately.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentPhase {
    Idle,
    Busy,
}

#[derive(Debug, Clone)]
pub struct Decision {
    pub phase: AgentPhase,
    pub text: String,
    #[allow(dead_code)]
    pub busy_elapsed_ms: u128,
    #[allow(dead_code)]
    pub queue_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    SendNow,
    Interrupt,
    Enqueue,
    Stream,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::SendNow => "send_now",
            Verdict::Interrupt => "interrupt",
            Verdict::Enqueue => "enqueue",
            Verdict::Stream => "stream",
        }
    }
}

pub trait Arbiter: Send {
    fn decide(&self, ctx: &Decision) -> Verdict;
    fn name(&self) -> &str;
}

// ponytail: "no" and "actually" left out on purpose — they open ordinary
// instructions ("no tests needed", "actually use postgres") far too often to be
// safe barge-in triggers. Users who want them can add them via interrupt_keywords
// in .delphin.toml.
pub const DEFAULT_INTERRUPT_KEYWORDS: &[&str] = &[
    "stop",
    "wait",
    "cancel",
    "abort",
    "hold on",
    "nevermind",
    "never mind",
    "scratch that",
    "urgent",
    "halt",
];

pub struct HeuristicArbiter {
    interrupt_keywords: Vec<String>,
    stream_while_busy: bool,
}

impl HeuristicArbiter {
    pub fn new(interrupt_keywords: Vec<String>) -> Self {
        Self::new_with_mode(interrupt_keywords, false)
    }

    pub fn new_with_mode(interrupt_keywords: Vec<String>, stream_while_busy: bool) -> Self {
        Self {
            interrupt_keywords: interrupt_keywords
                .into_iter()
                .map(|k| k.to_lowercase())
                .collect(),
            stream_while_busy,
        }
    }

    #[allow(dead_code)] // used by tests and downstream callers
    pub fn with_defaults() -> Self {
        Self::new(
            DEFAULT_INTERRUPT_KEYWORDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )
    }

    fn signals_interrupt(&self, text: &str) -> bool {
        let hay = text.to_lowercase();
        self.interrupt_keywords
            .iter()
            .any(|kw| contains_word(&hay, kw))
    }
}

impl Arbiter for HeuristicArbiter {
    fn decide(&self, ctx: &Decision) -> Verdict {
        match ctx.phase {
            AgentPhase::Idle => Verdict::SendNow,
            AgentPhase::Busy => {
                if self.signals_interrupt(&ctx.text) {
                    Verdict::Interrupt
                } else if self.stream_while_busy {
                    Verdict::Stream
                } else {
                    Verdict::Enqueue
                }
            }
        }
    }

    fn name(&self) -> &str {
        "heuristic"
    }
}

/// Like the heuristic, but also treats **questions** as interrupt-worthy: while
/// the agent is busy, a question (or an urgency keyword) barges in; plain
/// statements / added instructions queue. Rationale: a question usually needs an
/// answer before the agent's current work is even useful.
pub struct QuestionArbiter {
    heuristic: HeuristicArbiter,
}

impl QuestionArbiter {
    #[allow(dead_code)] // used by tests and downstream callers
    pub fn new(interrupt_keywords: Vec<String>) -> Self {
        Self::new_with_mode(interrupt_keywords, false)
    }

    pub fn new_with_mode(interrupt_keywords: Vec<String>, stream_while_busy: bool) -> Self {
        Self {
            heuristic: HeuristicArbiter::new_with_mode(interrupt_keywords, stream_while_busy),
        }
    }
}

/// Heuristic question detector: ends with "?", or starts with a wh-/aux word.
fn looks_like_question(text: &str) -> bool {
    let t = text.trim();
    if t.ends_with('?') {
        return true;
    }
    let first = t.split_whitespace().next().unwrap_or("").to_lowercase();
    matches!(
        first.as_str(),
        "who"
            | "what"
            | "why"
            | "how"
            | "when"
            | "where"
            | "which"
            | "whose"
            | "whom"
            | "is"
            | "are"
            | "do"
            | "does"
            | "did"
            | "can"
            | "could"
            | "should"
            | "would"
            | "will"
            | "am"
            | "was"
            | "were"
            | "have"
            | "has"
    )
}

impl Arbiter for QuestionArbiter {
    fn decide(&self, ctx: &Decision) -> Verdict {
        match ctx.phase {
            AgentPhase::Idle => Verdict::SendNow,
            AgentPhase::Busy => {
                if self.heuristic.signals_interrupt(&ctx.text) || looks_like_question(&ctx.text) {
                    Verdict::Interrupt
                } else if self.heuristic.stream_while_busy {
                    Verdict::Stream
                } else {
                    Verdict::Enqueue
                }
            }
        }
    }

    fn name(&self) -> &str {
        "question"
    }
}

/// Which arbiter policy to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArbiterKind {
    Heuristic,
    Question,
}

impl ArbiterKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "heuristic" => Some(Self::Heuristic),
            "question" | "questions" => Some(Self::Question),
            _ => None,
        }
    }
}

/// Construct the selected arbiter policy.
pub fn build_arbiter(
    kind: ArbiterKind,
    interrupt_keywords: Vec<String>,
    stream_while_busy: bool,
) -> Box<dyn Arbiter> {
    match kind {
        ArbiterKind::Heuristic => Box::new(HeuristicArbiter::new_with_mode(
            interrupt_keywords,
            stream_while_busy,
        )),
        ArbiterKind::Question => Box::new(QuestionArbiter::new_with_mode(
            interrupt_keywords,
            stream_while_busy,
        )),
    }
}

/// Whole-word / whole-phrase containment (case assumed normalized). A match must
/// be bordered by string edges or non-alphanumeric chars so keywords do not fire
/// inside larger words ("stopgap" must not match "stop").
fn contains_word(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let bytes = hay.as_bytes();
    let mut start = 0;
    while let Some(pos) = hay[start..].find(needle) {
        let i = start + pos;
        let before_ok = i == 0 || !is_word_byte(bytes[i - 1]);
        let after = i + needle.len();
        let after_ok = after >= bytes.len() || !is_word_byte(bytes[after]);
        if before_ok && after_ok {
            return true;
        }
        start = i + 1;
        if start >= hay.len() {
            break;
        }
    }
    false
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn busy(text: &str) -> Decision {
        Decision {
            phase: AgentPhase::Busy,
            text: text.into(),
            busy_elapsed_ms: 1000,
            queue_len: 0,
        }
    }
    fn idle(text: &str) -> Decision {
        Decision {
            phase: AgentPhase::Idle,
            text: text.into(),
            busy_elapsed_ms: 0,
            queue_len: 0,
        }
    }

    #[test]
    fn idle_always_sends_now() {
        let a = HeuristicArbiter::with_defaults();
        assert_eq!(a.decide(&idle("anything")), Verdict::SendNow);
        assert_eq!(a.decide(&idle("stop")), Verdict::SendNow);
    }

    #[test]
    fn busy_queues_normal_prompts() {
        let a = HeuristicArbiter::with_defaults();
        assert_eq!(
            a.decide(&busy("also add a dark mode toggle")),
            Verdict::Enqueue
        );
        assert_eq!(
            a.decide(&busy("what is the capital of France?")),
            Verdict::Enqueue
        );
    }

    #[test]
    fn busy_interrupts_on_keyword() {
        let a = HeuristicArbiter::with_defaults();
        assert_eq!(a.decide(&busy("stop, wrong file")), Verdict::Interrupt);
        assert_eq!(
            a.decide(&busy("wait — use rust not go")),
            Verdict::Interrupt
        );
        assert_eq!(
            a.decide(&busy("actually never mind the tests")),
            Verdict::Interrupt
        );
        assert_eq!(a.decide(&busy("halt, wrong branch")), Verdict::Interrupt);
    }

    #[test]
    fn weak_words_do_not_interrupt_by_default() {
        let a = HeuristicArbiter::with_defaults();
        // ordinary instructions that merely open with a weak word must queue,
        // not barge in and kill the in-flight work.
        assert_eq!(a.decide(&busy("no tests needed")), Verdict::Enqueue);
        assert_eq!(a.decide(&busy("actually use postgres")), Verdict::Enqueue);
        // a genuine urgency word still interrupts, even mid-sentence.
        assert_eq!(a.decide(&busy("please stop the build")), Verdict::Interrupt);
    }

    #[test]
    fn keywords_match_whole_words_only() {
        let a = HeuristicArbiter::with_defaults();
        assert_eq!(a.decide(&busy("add a stopgap measure")), Verdict::Enqueue);
        assert_eq!(
            a.decide(&busy("nope handling looks fine")),
            Verdict::Enqueue
        );
    }

    #[test]
    fn custom_keywords_respected() {
        let a = HeuristicArbiter::new(vec!["pause".to_string()]);
        assert_eq!(a.decide(&busy("pause please")), Verdict::Interrupt);
        assert_eq!(a.decide(&busy("stop please")), Verdict::Enqueue);
    }

    #[test]
    fn question_arbiter_interrupts_questions_queues_commands() {
        let a = QuestionArbiter::new(
            DEFAULT_INTERRUPT_KEYWORDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        // questions barge in
        assert_eq!(
            a.decide(&busy("what database are we using?")),
            Verdict::Interrupt
        );
        assert_eq!(
            a.decide(&busy("how does the arbiter decide")),
            Verdict::Interrupt
        );
        // urgency keyword still interrupts
        assert_eq!(a.decide(&busy("stop that")), Verdict::Interrupt);
        // plain instructions queue
        assert_eq!(a.decide(&busy("also add a logging flag")), Verdict::Enqueue);
        // idle always sends now
        assert_eq!(a.decide(&idle("what is this?")), Verdict::SendNow);
    }

    #[test]
    fn question_detection() {
        assert!(looks_like_question("why did that fail?"));
        assert!(looks_like_question("How should we store this"));
        assert!(!looks_like_question("add a dark mode toggle"));
        assert!(!looks_like_question("rename the config flag"));
    }

    #[test]
    fn arbiter_kind_parse() {
        assert_eq!(
            ArbiterKind::parse("heuristic"),
            Some(ArbiterKind::Heuristic)
        );
        assert_eq!(ArbiterKind::parse("question"), Some(ArbiterKind::Question));
        assert_eq!(ArbiterKind::parse("questions"), Some(ArbiterKind::Question));
        assert_eq!(ArbiterKind::parse("nope"), None);
        assert_eq!(
            build_arbiter(ArbiterKind::Question, vec![], false).name(),
            "question"
        );
    }

    #[test]
    fn live_mode_streams_normal_busy_prompts() {
        let a = HeuristicArbiter::new_with_mode(
            DEFAULT_INTERRUPT_KEYWORDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            true,
        );
        assert_eq!(a.decide(&busy("also add logging")), Verdict::Stream);
        assert_eq!(a.decide(&busy("stop wrong file")), Verdict::Interrupt);
        assert_eq!(a.decide(&idle("also add logging")), Verdict::SendNow);
    }

    #[test]
    fn question_live_mode_still_interrupts_questions() {
        let a = QuestionArbiter::new_with_mode(
            DEFAULT_INTERRUPT_KEYWORDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            true,
        );
        assert_eq!(a.decide(&busy("also add logging")), Verdict::Stream);
        assert_eq!(a.decide(&busy("what file is this?")), Verdict::Interrupt);
        assert_eq!(a.decide(&busy("stop wrong file")), Verdict::Interrupt);
    }
}
