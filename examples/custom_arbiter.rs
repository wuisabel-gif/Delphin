use delphin::arbiter::{AgentPhase, Arbiter, Decision, Verdict};

/// Example policy that prioritizes short corrections while protecting longer
/// additions until the current turn completes.
struct ShortCorrectionArbiter;

impl Arbiter for ShortCorrectionArbiter {
    fn decide(&self, context: &Decision) -> Verdict {
        match context.phase {
            AgentPhase::Idle => Verdict::SendNow,
            AgentPhase::Busy if context.text.split_whitespace().count() <= 3 => Verdict::Interrupt,
            AgentPhase::Busy => Verdict::Enqueue,
        }
    }

    fn name(&self) -> &str {
        "short-correction"
    }
}

fn main() {
    let policy = ShortCorrectionArbiter;
    let decision = Decision {
        phase: AgentPhase::Busy,
        text: "wrong file".to_string(),
        busy_elapsed_ms: 500,
        queue_len: 0,
    };
    println!("{}: {}", policy.name(), policy.decide(&decision).as_str());
}
