use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::model::{ContentBlock, Message, Role};

#[derive(Debug, Clone)]
pub struct ContextManager {
    pub max_messages: usize,
    pub preserve_recent: usize,
    /// Maximum characters retained from any single tool output before it is
    /// truncated with a marker. Older tool outputs are the main source of
    /// context bloat, so capping them keeps the window usable without losing
    /// the recent, relevant tail.
    pub max_tool_output_chars: usize,
    pub token_estimate: Arc<AtomicUsize>,
}

impl Default for ContextManager {
    fn default() -> Self {
        Self {
            max_messages: 64,
            preserve_recent: 12,
            max_tool_output_chars: 4000,
            token_estimate: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ContextManager {
    pub fn new(max_messages: usize, preserve_recent: usize) -> Self {
        Self {
            max_messages: max_messages.max(preserve_recent + 1),
            preserve_recent,
            max_tool_output_chars: 4000,
            token_estimate: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Shrink every `ToolResponse` content that exceeds `max_tool_output_chars`,
    /// keeping the head and a truncation marker. This runs before message-count
    /// compaction so that oversized outputs do not inflate the token estimate
    /// unnecessarily. Returns the number of blocks truncated.
    pub fn truncate_tool_outputs(&self, messages: &mut [Message]) -> usize {
        let limit = self.max_tool_output_chars;
        let mut truncated = 0usize;
        for msg in messages.iter_mut() {
            for block in msg.content.iter_mut() {
                if let ContentBlock::ToolResponse { content, .. } = block {
                    if content.chars().count() > limit {
                        let head: String = content.chars().take(limit).collect();
                        let removed = content.chars().count() - limit;
                        *content = format!(
                            "{}\n…[truncated {} characters by context manager]",
                            head, removed
                        );
                        truncated += 1;
                    }
                }
            }
        }
        truncated
    }

    pub fn estimate_tokens(&self, messages: &[Message]) -> usize {
        let mut total = 0usize;
        for msg in messages {
            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => {
                        total = total.saturating_add(text.len().saturating_add(4) / 4);
                    }
                    ContentBlock::ToolRequest {
                        name, arguments, ..
                    } => {
                        total = total.saturating_add(name.len().saturating_add(4) / 4);
                        total = total.saturating_add(
                            serde_json::to_string(arguments)
                                .map(|s| s.len().saturating_add(4) / 4)
                                .unwrap_or(0),
                        );
                    }
                    ContentBlock::ToolResponse {
                        content, is_error, ..
                    } => {
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
        // Always cap oversized tool outputs first; this is cheap and prevents
        // a single huge output from dominating the window even when we are
        // below the message-count threshold.
        self.truncate_tool_outputs(messages);

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

    pub fn maybe_compact_by_token_threshold(
        &self,
        messages: &mut Vec<Message>,
        max_tokens: usize,
    ) -> CompactionResult {
        self.truncate_tool_outputs(messages);
        self.token_estimate
            .store(self.estimate_tokens(messages), Ordering::Relaxed);
        if max_tokens == 0 || self.token_estimate() < max_tokens {
            return CompactionResult::Noop;
        }

        let original_len = messages.len();
        let keep = self.preserve_recent.min(messages.len());
        if keep == 0 || messages.len() <= keep {
            return CompactionResult::Noop;
        }

        let removed: Vec<Message> = messages.drain(0..messages.len() - keep).collect();
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
                    ContentBlock::ToolResponse {
                        content, is_error, ..
                    } => {
                        let prefix = if *is_error { "tool-error" } else { "tool" };
                        Some(format!("{}({})", prefix, content))
                    }
                    ContentBlock::ToolRequest {
                        name, arguments, ..
                    } => Some(format!("{}({})", name, arguments)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ContentBlock, Message, Role};

    fn tool_response(content: &str) -> Message {
        Message {
            id: "m".to_string(),
            role: Role::Tool,
            content: vec![ContentBlock::ToolResponse {
                id: "t".to_string(),
                content: content.to_string(),
                is_error: false,
            }],
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_truncate_tool_outputs_caps_oversized() {
        let mut cm = ContextManager::default();
        cm.max_tool_output_chars = 10;
        // Large enough that the retained head + marker is still far shorter.
        let long = "x".repeat(500);
        let mut messages = vec![tool_response(&long)];
        let truncated = cm.truncate_tool_outputs(&mut messages);
        assert_eq!(truncated, 1);
        if let ContentBlock::ToolResponse { content, .. } = &messages[0].content[0] {
            assert!(content.contains("[truncated"));
            // Truncated content is much smaller than the original 500 chars.
            assert!(content.chars().count() < 500);
            // Retained head is present.
            assert!(content.starts_with("xxxxxxxxxx"));
        } else {
            panic!("expected ToolResponse");
        }
    }

    #[test]
    fn test_truncate_tool_outputs_leaves_short_untouched() {
        let cm = ContextManager::default(); // limit 4000
        let mut messages = vec![tool_response("short output")];
        let truncated = cm.truncate_tool_outputs(&mut messages);
        assert_eq!(truncated, 0);
        if let ContentBlock::ToolResponse { content, .. } = &messages[0].content[0] {
            assert_eq!(content, "short output");
        }
    }

    #[test]
    fn test_maybe_compact_truncates_even_below_threshold() {
        let mut cm = ContextManager::default();
        cm.max_tool_output_chars = 5;
        let long = "y".repeat(100);
        let mut messages = vec![tool_response(&long)];
        // Below max_messages, but should still truncate the tool output.
        let result = cm.maybe_compact(&mut messages);
        assert_eq!(result, CompactionResult::Noop);
        if let ContentBlock::ToolResponse { content, .. } = &messages[0].content[0] {
            assert!(content.contains("[truncated"));
        }
    }
}
