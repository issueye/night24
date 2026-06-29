use std::future::Future;
use std::pin::Pin;

use async_trait::async_trait;
use async_stream::try_stream;
use chrono::Utc;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::{ContentBlock, Message, Role, Tool};
use crate::provider::{MessageStream, ModelConfig, Provider, ProviderError, ProviderUsage};

#[derive(Debug, Clone)]
pub struct OllamaProvider {
    pub base_url: String,
    pub model: String,
}

impl OllamaProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: "llama3.2".to_string(),
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
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

            let mut ollama_messages = vec![OllamaMessage {
                role: "system".to_string(),
                content: Some(system.to_string()),
                tool_calls: None,
            }];

            for msg in messages {
                ollama_messages.push(ollama_message_from_goose(msg));
            }

            let body = serde_json::json!({
                "model": model,
                "messages": ollama_messages,
                "stream": true,
                "tools": if tools.is_empty() { None } else {
                    Some(tools.iter().map(|t| {
                        serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.parameters
                            }
                        })
                    }).collect::<Vec<_>>())
                }
            });

            let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));
            let response = client
                .post(url)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await?;
                return Err(ProviderError::Message(format!(
                    "Ollama API error {}: {}",
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
                        if line.is_empty() {
                            continue;
                        }
                        let chunk: OllamaChatChunk = match serde_json::from_str(line) {
                            Ok(c) => c,
                            Err(_) => continue,
                        };

                        if let Some(msg) = chunk.message {
                            if !msg.role.is_empty() {
                                accumulated.role = Some(msg.role);
                            }
                            if let Some(content) = msg.content {
                                accumulated.content.push_str(&content);
                            }
                            if let Some(tool_calls) = msg.tool_calls {
                                for tc in tool_calls {
                                    accumulated.ollama_tool_calls.push(tc);
                                }
                            }
                        }

                        if chunk.done {
                            if let Some(fin_msg) = accumulated.finish() {
                                yield (Some(fin_msg), ProviderUsage::default());
                            }
                            accumulated = AccumulatedMessage::default();
                        }
                    }
                }
            };

            Ok(Box::pin(output) as MessageStream)
        })
    }
}

#[derive(Debug, Default)]
struct AccumulatedMessage {
    role: Option<String>,
    content: String,
    #[allow(dead_code)]
    ollama_tool_calls: Vec<OllamaToolCall>,
}

impl AccumulatedMessage {
    fn finish(self) -> Option<Message> {
        if self.content.is_empty() && self.ollama_tool_calls.is_empty() {
            return None;
        }

        let mut content_blocks = vec![];

        if !self.content.is_empty() {
            content_blocks.push(ContentBlock::Text { text: self.content });
        }

        let mut tool_requests = vec![];
        for (idx, tc) in self.ollama_tool_calls.into_iter().enumerate() {
            if let Some(func) = tc.function {
                let args = match serde_json::from_str(&func.arguments.unwrap_or_default()) {
                    Ok(v) => v,
                    Err(_) => serde_json::json!({}),
                };
                tool_requests.push(ContentBlock::ToolRequest {
                    id: format!("tc-{}", idx),
                    name: func.name,
                    arguments: args,
                });
            }
        }
        content_blocks.extend(tool_requests);

        if content_blocks.is_empty() {
            return None;
        }

        let role = match self.role.as_deref() {
            Some("assistant") => Role::Assistant,
            Some("user") => Role::User,
            Some("system") => Role::System,
            Some("tool") => Role::Tool,
            _ => Role::Assistant,
        };

        Some(Message {
            id: Uuid::new_v4().to_string(),
            role,
            content: content_blocks,
            created_at: Utc::now(),
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OllamaMessage {
    role: String,
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OllamaToolCall {
    #[serde(rename = "function")]
    function: Option<OllamaFunction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OllamaFunction {
    name: String,
    arguments: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OllamaChatChunk {
    message: Option<OllamaMessage>,
    done: bool,
}

fn ollama_message_from_goose(msg: &Message) -> OllamaMessage {
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::System => "system",
        Role::Tool => "tool",
    };

    let mut text_parts = vec![];
    let mut tool_calls = vec![];

    for block in &msg.content {
        match block {
            ContentBlock::Text { text } => text_parts.push(text.clone()),
            ContentBlock::ToolRequest { id: _, name, arguments } => {
                tool_calls.push(OllamaToolCall {
                    function: Some(OllamaFunction {
                        name: name.clone(),
                        arguments: Some(serde_json::to_string(arguments).unwrap_or_default()),
                    }),
                });
            }
            ContentBlock::ToolResponse { id: _, content, is_error: _ } => {
                text_parts.push(format!("[tool result] {}", content));
            }
            ContentBlock::Thinking { text } => {
                text_parts.push(format!("[thinking] {}", text));
            }
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join("\n"))
    };

    OllamaMessage {
        role: role.to_string(),
        content,
        tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
    }
}
