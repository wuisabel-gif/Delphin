//! The arbiter decides what to do with a user prompt that arrives while the
//! agent is mid-flight — the heart of delphin. It is a trait so the policy is
//! swappable; v1 ships the deterministic [`HeuristicArbiter`].
//!
//! Default policy: **the in-flight thinking is protected.** While the agent is
//! busy a new prompt is *queued*, UNLESS the user signals urgency with an
//! interrupt word ("stop", "wait", "no", "actually", …), in which case delphin
//! barges in. When the agent is idle, everything is sent immediately.

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
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::SendNow => "send_now",
            Verdict::Interrupt => "interrupt",
            Verdict::Enqueue => "enqueue",
        }
    }
}

pub trait Arbiter: Send {
    fn decide(&self, ctx: &Decision) -> Verdict;
    fn name(&self) -> &str;
}

pub const DEFAULT_INTERRUPT_KEYWORDS: &[&str] = &[
    "stop",
    "wait",
    "no",
    "cancel",
    "abort",
    "actually",
    "hold on",
    "nevermind",
    "never mind",
    "scratch that",
    "urgent",
    "halt",
];

pub struct HeuristicArbiter {
    interrupt_keywords: Vec<String>,
}

impl HeuristicArbiter {
    pub fn new(interrupt_keywords: Vec<String>) -> Self {
        Self {
            interrupt_keywords: interrupt_keywords
                .into_iter()
                .map(|k| k.to_lowercase())
                .collect(),
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
        assert_eq!(a.decide(&busy("NO don't push")), Verdict::Interrupt);
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
}
