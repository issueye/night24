use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::model::{ContentBlock, Message, Role};

#[derive(Debug, Clone)]
pub struct ContextManager {
    pub max_messages: usize,
    pub preserve_recent: usize,
    pub token_estimate: Arc<AtomicUsize>,
}

impl Default for ContextManager {
    fn default() -> Self {
        Self {
            max_messages: 64,
            preserve_recent: 12,
            token_estimate: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ContextManager {
    pub fn new(max_messages: usize, preserve_recent: usize) -> Self {
        Self {
            max_messages: max_messages.max(preserve_recent + 1),
            preserve_recent,
            token_estimate: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn estimate_tokens(&self, messages: &[Message]) -> usize {
        let mut total = 0usize;
        for msg in messages {
            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => {
                        total = total.saturating_add(text.len().saturating_add(4) / 4);
                    }
                    ContentBlock::ToolRequest { name, arguments, .. } => {
                        total = total.saturating_add(name.len().saturating_add(4) / 4);
                        total = total.saturating_add(
                            serde_json::to_string(arguments)
                                .map(|s| s.len().saturating_add(4) / 4)
                                .unwrap_or(0),
                        );
                    }
                    ContentBlock::ToolResponse { content, is_error, .. } => {
                        total = total.saturating_add(content.len().saturating_add(4) / 4);
                        if *is_error {
                            total = total.saturating_add(10);
                        }
                    }
                    ContentBlock::Thinking { text } => {
                        total = total.saturating_add(text.len().saturating_add(4) / 4);
                    }
                }
            }
        }
        total
    }

    pub fn maybe_compact(&self, messages: &mut Vec<Message>) -> CompactionResult {
        let original_len = messages.len();
        if messages.len() <= self.max_messages {
            self.token_estimate
                .store(self.estimate_tokens(messages), Ordering::Relaxed);
            return CompactionResult::Noop;
        }

        let keep = self.preserve_recent.min(messages.len());
        let mut removed = Vec::new();

        if keep > 0 && messages.len() > keep {
            removed = messages.drain(0..messages.len() - keep).collect();
        }

        if !removed.is_empty() {
            let summary = Self::summarize(&removed);
            if !summary.is_empty() {
                messages.insert(
                    0,
                    Message {
                        id: uuid::Uuid::new_v4().to_string(),
                        role: Role::System,
                        content: vec![ContentBlock::Text { text: summary }],
                        created_at: chrono::Utc::now(),
                    },
                );
            }
        }

        self.token_estimate
            .store(self.estimate_tokens(messages), Ordering::Relaxed);

        CompactionResult::Compacted {
            removed: original_len - messages.len(),
            current: messages.len(),
        }
    }

    fn summarize(removed: &[Message]) -> String {
        let mut summary = String::from("[compacted context]");
        for msg in removed.iter().rev().take(8) {
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::System => "system",
                Role::Tool => "tool",
            };
            let text = msg
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.clone()),
                    ContentBlock::ToolResponse { content, is_error, .. } => {
                        let prefix = if *is_error { "tool-error" } else { "tool" };
                        Some(format!("{}({})", prefix, content))
                    }
                    ContentBlock::ToolRequest { name, arguments, .. } => {
                        Some(format!("{}({})", name, arguments))
                    }
                    ContentBlock::Thinking { text } => Some(format!("thinking: {}", text)),
                })
                .collect::<Vec<_>>()
                .join("; ");

            if text.is_empty() {
                continue;
            }

            let snippet = if text.len() > 120 {
                format!("{}...", &text[..120])
            } else {
                text
            };
            summary.push_str(&format!("\n- {}: {}", role, snippet));
        }
        summary
    }

    pub fn token_estimate(&self) -> usize {
        self.token_estimate.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionResult {
    Noop,
    Compacted { removed: usize, current: usize },
}

impl std::fmt::Display for CompactionResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompactionResult::Noop => write!(f, "no compaction needed"),
            CompactionResult::Compacted { removed, current } => {
                write!(f, "compacted {} messages, now {}", removed, current)
            }
        }
    }
}
