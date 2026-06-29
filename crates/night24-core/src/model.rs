use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::User => write!(f, "user"),
            Role::Assistant => write!(f, "assistant"),
            Role::System => write!(f, "system"),
            Role::Tool => write!(f, "tool"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolRequest { id: String, name: String, arguments: serde_json::Value },
    ToolResponse { id: String, content: String, is_error: bool },
    Thinking { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Message {
    pub id: String,
    pub role: Role,
    pub content: Vec<ContentBlock>,
    pub created_at: DateTime<Utc>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            created_at: Utc::now(),
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::Text { text: text.into() }],
            created_at: Utc::now(),
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::System,
            content: vec![ContentBlock::Text { text: text.into() }],
            created_at: Utc::now(),
        }
    }

    pub fn tool(id: impl Into<String>, name: impl Into<String>, arguments: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::Tool,
            content: vec![ContentBlock::ToolRequest {
                id: id.into(),
                name: name.into(),
                arguments,
            }],
            created_at: Utc::now(),
        }
    }

    pub fn tool_response(id: impl Into<String>, content: impl Into<String>, is_error: bool) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::Tool,
            content: vec![ContentBlock::ToolResponse {
                id: id.into(),
                content: content.into(),
                is_error,
            }],
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Tool {
    pub name: String,
    pub description: String,
    #[serde(rename = "input_schema")]
    pub parameters: serde_json::Value,
}
