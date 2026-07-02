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
pub struct OpenAIResponsesProvider {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl OpenAIResponsesProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o".to_string(),
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
impl Provider for OpenAIResponsesProvider {
    fn name(&self) -> &str {
        "openai-responses"
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

            let mut input_items: Vec<ResponsesInputItem> = vec![];
            if !system.is_empty() {
                input_items.push(ResponsesInputItem {
                    r#type: "message".to_string(),
                    role: "system".to_string(),
                    content: system.to_string(),
                });
            }
            for msg in messages {
                input_items.push(responses_input_item_from_goose(msg));
            }

            let body = ResponsesRequest {
                model,
                input: input_items,
                tools: if tools.is_empty() {
                    None
                } else {
                    Some(
                        tools
                            .iter()
                            .map(|t| ResponsesTool {
                                r#type: "function".to_string(),
                                name: t.name.clone(),
                                description: t.description.clone(),
                                parameters: t.parameters.clone(),
                            })
                            .collect(),
                    )
                },
                temperature: model_config.temperature,
                max_output_tokens: model_config.max_tokens,
                stream: true,
            };

            let url = format!("{}/responses", self.base_url.trim_end_matches('/'));
            let response = client
                .post(url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await?;
                return Err(ProviderError::Message(format!(
                    "OpenAI Responses API error {}: {}",
                    status, text
                )));
            }

            let stream = response.bytes_stream();
            let output = try_stream! {
                let mut stream = std::pin::pin!(stream);
                let mut accumulated = AccumulatedMessage::default();

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

                        let event: ResponsesEvent = match serde_json::from_str(data) {
                            Ok(e) => e,
                            Err(_) => continue,
                        };

                        match event.r#type.as_deref() {
                            Some("response.created") => {}
                            Some("response.output_item.added") => {
                                if let Some(item) = event.item {
                                    match item.r#type.as_str() {
                                        "message" => {
                                            accumulated.role = Some("assistant".to_string());
                                        }
                                        "function_call" => {
                                            accumulated.tool_calls.push(ResponsesToolCall {
                                                id: item.id.unwrap_or_default(),
                                                name: item.name.unwrap_or_default(),
                                                arguments: item.arguments.unwrap_or_default(),
                                                call_id: item.call_id.unwrap_or_default(),
                                            });
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Some("response.output_text.delta") => {
                                if let Some(delta) = event.delta {
                                    accumulated.content.push_str(&delta);
                                    if let Some(msg) = accumulated.snapshot() {
                                        yield (Some(msg), ProviderUsage::default());
                                    }
                                }
                            }
                            Some("response.function_call_arguments.delta") => {
                                if let Some(delta) = event.delta {
                                    accumulated.tool_call_arguments.push_str(&delta);
                                }
                            }
                            Some("response.output_item.done") => {
                                if let Some(item) = event.item {
                                    if item.r#type == "function_call" {
                                        if let Some(idx) = accumulated.tool_calls.iter().position(|tc| tc.id == item.id.clone().unwrap_or_default()) {
                                            if let Some(tc) = accumulated.tool_calls.get_mut(idx) {
                                                tc.arguments = accumulated.tool_call_arguments.clone();
                                            }
                                        }
                                        accumulated.tool_call_arguments.clear();
                                    }
                                }
                            }
                            Some("response.completed") => {
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

#[derive(Debug, Clone)]
struct AccumulatedMessage {
    id: String,
    role: Option<String>,
    content: String,
    tool_calls: Vec<ResponsesToolCall>,
    tool_call_arguments: String,
}

impl Default for AccumulatedMessage {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: None,
            content: String::new(),
            tool_calls: Vec::new(),
            tool_call_arguments: String::new(),
        }
    }
}

impl AccumulatedMessage {
    fn snapshot(&self) -> Option<Message> {
        self.clone().finish()
    }

    fn finish(self) -> Option<Message> {
        let mut blocks: Vec<ContentBlock> = vec![];
        if !self.content.is_empty() {
            blocks.push(ContentBlock::Text { text: self.content });
        }
        for tc in self.tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));
            blocks.push(ContentBlock::ToolRequest {
                id: tc.call_id,
                name: tc.name,
                arguments: args,
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
            id: self.id,
            role,
            content: blocks,
            created_at: Utc::now(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
struct ResponsesRequest {
    model: String,
    input: Vec<ResponsesInputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ResponsesTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    stream: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ResponsesInputItem {
    #[serde(rename = "type")]
    r#type: String,
    role: String,
    content: String,
}

#[derive(Debug, Clone, Serialize)]
struct ResponsesTool {
    #[serde(rename = "type")]
    r#type: String,
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ResponsesEvent {
    #[serde(rename = "type")]
    r#type: Option<String>,
    #[serde(default)]
    delta: Option<String>,
    #[serde(default)]
    item: Option<ResponsesItem>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResponsesItem {
    #[serde(rename = "type")]
    r#type: String,
    id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
    call_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ResponsesToolCall {
    id: String,
    call_id: String,
    name: String,
    arguments: String,
}

fn responses_input_item_from_goose(msg: &Message) -> ResponsesInputItem {
    let (role, content) = match msg.role {
        Role::User => ("user", goose_content_to_string(&msg.content)),
        Role::Assistant => ("assistant", goose_content_to_string(&msg.content)),
        Role::System => ("system", goose_content_to_string(&msg.content)),
        Role::Tool => ("tool", goose_content_to_string(&msg.content)),
    };
    ResponsesInputItem {
        r#type: "message".to_string(),
        role: role.to_string(),
        content,
    }
}

fn goose_content_to_string(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.clone(),
            ContentBlock::ToolRequest {
                id: _,
                name,
                arguments,
            } => {
                format!("[tool request] {}: {}", name, arguments)
            }
            ContentBlock::ToolResponse {
                id: _,
                content,
                is_error: _,
            } => {
                format!("[tool result] {}", content)
            }
            ContentBlock::Thinking { text } => format!("[thinking] {}", text),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
