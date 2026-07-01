use std::future::Future;
use std::pin::Pin;

use async_stream::try_stream;
use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::{ContentBlock, Message, Role, Tool};
use crate::provider::{MessageStream, ModelConfig, Provider, ProviderError, ProviderUsage};

#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.anthropic.com/v1".to_string(),
            model: "claude-3-5-haiku-20241022".to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn stream<'a>(
        &'a self,
        model_config: &'a ModelConfig,
        system: &'a str,
        messages: &'a [Message],
        tools: &'a [Tool],
    ) -> Pin<Box<dyn Future<Output = Result<MessageStream, ProviderError>> + Send + 'a>> {
        Box::pin(async move {
            let client = Client::new();
            let model = if model_config.model.is_empty() {
                self.model.clone()
            } else {
                model_config.model.clone()
            };

            let mut anthropic_messages: Vec<AnthropicMessage> = vec![];
            for msg in messages {
                anthropic_messages.push(anthropic_message_from_goose(msg));
            }

            let anthropic_tools: Vec<AnthropicTool> = if tools.is_empty() {
                vec![]
            } else {
                tools
                    .iter()
                    .map(|t| AnthropicTool {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        input_schema: t.parameters.clone(),
                    })
                    .collect()
            };

            let body = AnthropicChatRequest {
                model,
                system: if system.is_empty() {
                    None
                } else {
                    Some(system.to_string())
                },
                messages: anthropic_messages,
                tools: if anthropic_tools.is_empty() {
                    None
                } else {
                    Some(anthropic_tools)
                },
                max_tokens: model_config.max_tokens.unwrap_or(1024),
                temperature: model_config.temperature,
                stream: true,
            };

            let url = format!("{}/messages", self.base_url.trim_end_matches('/'));
            let response = client
                .post(url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await?;
                return Err(ProviderError::Message(format!(
                    "Anthropic API error {}: {}",
                    status, text
                )));
            }

            let stream = response.bytes_stream();
            let output = try_stream! {
                let mut accumulated = AccumulatedMessage::default();
                let mut stream = std::pin::pin!(stream);

                while let Some(chunk) = stream.next().await {
                    let bytes = chunk?;
                    let text = String::from_utf8_lossy(&bytes);
                    for line in text.lines() {
                        let line = line.trim();
                        if line.is_empty() || !line.starts_with("data: ") {
                            continue;
                        }
                        let data = &line[6..];
                        if data == "[DONE]" {
                            if let Some(msg) = accumulated.finish() {
                                yield (Some(msg), ProviderUsage::default());
                            }
                            return;
                        }

                        let event: AnthropicEvent = match serde_json::from_str(data) {
                            Ok(e) => e,
                            Err(_) => continue,
                        };

                        match event.r#type.as_deref() {
                            Some("content_block_delta") => {
                                if let Some(delta) = event.delta {
                                    match delta.r#type.as_deref() {
                                        Some("text_delta") => {
                                            accumulated.content.push_str(&delta.text.unwrap_or_default());
                                        }
                                        Some("input_json_delta") => {
                                            accumulated.tool_call_arguments.push_str(&delta.partial_json.unwrap_or_default());
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Some("content_block_start") => {
                                if let Some(content_block) = event.content_block {
                                    match content_block.r#type.as_deref() {
                                        Some("text") => {
                                            accumulated.role = Some("assistant".to_string());
                                        }
                                        Some("tool_use") => {
                                            accumulated.tool_calls.push(AnthropicToolCall {
                                                id: content_block.id.unwrap_or_default(),
                                                name: content_block.name.unwrap_or_default(),
                                                input: content_block.input.unwrap_or(serde_json::json!({})),
                                            });
                                            accumulated.tool_call_arguments.clear();
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Some("message_stop") => {
                                if let Some(msg) = accumulated.finish() {
                                    yield (Some(msg), ProviderUsage::default());
                                }
                                accumulated = AccumulatedMessage::default();
                            }
                            _ => {}
                        }
                    }
                }
            };

            Ok(Box::pin(output) as MessageStream)
        })
    }
}

#[derive(Debug, Clone, Default)]
struct AccumulatedMessage {
    role: Option<String>,
    content: String,
    tool_calls: Vec<AnthropicToolCall>,
    tool_call_arguments: String,
}

impl AccumulatedMessage {
    fn finish(self) -> Option<Message> {
        let mut blocks: Vec<ContentBlock> = vec![];
        if !self.content.is_empty() {
            blocks.push(ContentBlock::Text { text: self.content });
        }
        for tc in self.tool_calls {
            blocks.push(ContentBlock::ToolRequest {
                id: tc.id,
                name: tc.name,
                arguments: tc.input,
            });
        }
        if blocks.is_empty() {
            return None;
        }
        let role = match self.role.as_deref() {
            Some("assistant") => Role::Assistant,
            Some("user") => Role::User,
            Some("system") => Role::System,
            _ => Role::Assistant,
        };
        Some(Message {
            id: Uuid::new_v4().to_string(),
            role,
            content: blocks,
            created_at: Utc::now(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicChatRequest {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicContentPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum AnthropicContentPart {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct AnthropicEvent {
    #[serde(rename = "type")]
    r#type: Option<String>,
    #[serde(default)]
    delta: Option<AnthropicDelta>,
    #[serde(default)]
    content_block: Option<AnthropicContentBlock>,
}

#[derive(Debug, Clone, Deserialize)]
struct AnthropicDelta {
    #[serde(rename = "type")]
    r#type: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    r#type: Option<String>,
    id: Option<String>,
    name: Option<String>,
    input: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
struct AnthropicToolCall {
    id: String,
    name: String,
    input: serde_json::Value,
}

fn anthropic_message_from_goose(msg: &Message) -> AnthropicMessage {
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::System => "user",
        Role::Tool => "user",
    };

    let mut parts: Vec<AnthropicContentPart> = vec![];
    for block in &msg.content {
        match block {
            ContentBlock::Text { text } => {
                parts.push(AnthropicContentPart::Text { text: text.clone() });
            }
            ContentBlock::ToolRequest {
                id,
                name,
                arguments,
            } => {
                parts.push(AnthropicContentPart::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: arguments.clone(),
                });
            }
            ContentBlock::ToolResponse {
                id,
                content,
                is_error: _,
            } => {
                parts.push(AnthropicContentPart::ToolResult {
                    tool_use_id: id.clone(),
                    content: content.clone(),
                });
            }
            ContentBlock::Thinking { text } => {
                parts.push(AnthropicContentPart::Text { text: text.clone() });
            }
        }
    }

    AnthropicMessage {
        role: role.to_string(),
        content: parts,
    }
}
