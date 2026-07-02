//! A small FIFO of prompts waiting for the agent to become idle.
//!
//! Interrupt-worthy prompts never land here — they are sent immediately by the
//! supervisor. This queue holds the "can wait" prompts, drained one at a time
//! (each drained prompt makes the agent busy again, so we release them singly).

use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct QueuedPrompt {
    pub id: u64,
    pub text: String,
    /// Group id minted when the prompt was typed; reused on release so the queued
    /// prompt, its release row, and the resulting reply all share one group.
    pub group: u64,
}

#[derive(Default)]
pub struct PromptQueue {
    items: VecDeque<QueuedPrompt>,
    next_id: u64,
}

impl PromptQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, text: String, group: u64) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.items.push_back(QueuedPrompt { id, text, group });
        id
    }

    pub fn pop(&mut self) -> Option<QueuedPrompt> {
        self.items.pop_front()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[allow(dead_code)] // part of the queue's public surface; used by tests
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_order_and_ids() {
        let mut q = PromptQueue::new();
        assert!(q.is_empty());
        assert_eq!(q.push("first".into(), 10), 1);
        assert_eq!(q.push("second".into(), 11), 2);
        assert_eq!(q.len(), 2);
        let first = q.pop().unwrap();
        assert_eq!(first.text, "first");
        assert_eq!(first.group, 10);
        assert_eq!(q.pop().unwrap().text, "second");
        assert!(q.pop().is_none());
    }
}
