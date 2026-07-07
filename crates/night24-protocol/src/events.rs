use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use night24_core::model::Message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEvent {
    pub run_id: String,
    pub seq: u64,
    #[serde(flatten)]
    pub kind: AgentEventKind,
    pub created_at: DateTime<Utc>,
}

impl AgentEvent {
    pub fn new(run_id: impl Into<String>, seq: u64, kind: AgentEventKind) -> Self {
        Self {
            run_id: run_id.into(),
            seq,
            kind,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum AgentEventKind {
    Message {
        message: Message,
    },
    MessageDelta {
        message_id: String,
        delta: String,
    },
    ToolStarted {
        tool_call_id: String,
        tool_name: String,
        summary: String,
        arguments: serde_json::Value,
    },
    ToolFinished {
        tool_call_id: String,
        tool_name: String,
        duration_ms: u64,
        summary: String,
        result_preview: String,
        is_error: bool,
    },
    ToolFailed {
        tool_call_id: String,
        tool_name: String,
        duration_ms: u64,
        error: EventError,
    },
    PermissionRequired {
        permission_id: String,
        tool_call_id: String,
        tool_name: String,
        risk: RiskLevel,
        summary: String,
        arguments: serde_json::Value,
        timeout_ms: u64,
    },
    RunOutput {
        source: String,
        stream: OutputStream,
        text: String,
    },
    DiffReady {
        files_changed: u32,
        insertions: u32,
        deletions: u32,
        summary: String,
    },
    SubAgentSession {
        subagent_id: String,
        child_run_id: String,
        parent_session_id: String,
        parent_run_id: String,
        name: String,
        task: String,
        status: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        messages: Vec<Message>,
    },
    Finish {
        status: FinishStatus,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        messages: Vec<Message>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
    },
    Error {
        code: String,
        message: String,
        recoverable: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishStatus {
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use night24_core::model::Message;

    #[test]
    fn message_event_uses_type_and_payload_shape() {
        let event = AgentEvent::new(
            "run-1",
            1,
            AgentEventKind::Message {
                message: Message::assistant("hello"),
            },
        );

        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["run_id"], "run-1");
        assert_eq!(value["seq"], 1);
        assert_eq!(value["type"], "message");
        assert_eq!(value["payload"]["message"]["role"], "assistant");
    }

    #[test]
    fn finish_event_can_omit_empty_messages_and_usage() {
        let event = AgentEvent::new(
            "run-1",
            2,
            AgentEventKind::Finish {
                status: FinishStatus::Completed,
                messages: Vec::new(),
                usage: None,
            },
        );

        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["type"], "finish");
        assert_eq!(value["payload"]["status"], "completed");
        assert!(value["payload"].get("messages").is_none());
        assert!(value["payload"].get("usage").is_none());
    }
}
