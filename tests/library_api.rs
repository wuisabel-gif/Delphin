use delphin::arbiter::{AgentPhase, Arbiter, Decision, Verdict};

struct ExternalPolicy;

impl Arbiter for ExternalPolicy {
    fn decide(&self, context: &Decision) -> Verdict {
        match context.phase {
            AgentPhase::Idle => Verdict::SendNow,
            AgentPhase::Busy => Verdict::Enqueue,
        }
    }

    fn name(&self) -> &str {
        "external"
    }
}

#[test]
fn external_crates_can_implement_the_arbiter_contract() {
    let policy = ExternalPolicy;
    let decision = Decision {
        phase: AgentPhase::Busy,
        text: "continue later".to_string(),
        busy_elapsed_ms: 100,
        queue_len: 0,
    };
    assert_eq!(policy.decide(&decision), Verdict::Enqueue);
}
