use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_stream::try_stream;
use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::model::{ContentBlock, Message, Role};
use crate::provider::{
    retry_error_suffix, retryable_status, sleep_before_retry, MessageStream, ModelConfig, Provider,
    ProviderError, ProviderUsage, MAX_REQUEST_RETRIES, PROVIDER_USER_AGENT,
};

#[derive(Debug, Clone)]
pub struct OpenAIProvider {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl OpenAIProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o-mini".to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = normalize_openai_base_url(&base_url.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn stream<'a>(
        &'a self,
        model_config: &'a ModelConfig,
        system: &'a str,
        messages: &'a [Message],
        tools: &'a [crate::model::Tool],
    ) -> Pin<Box<dyn Future<Output = Result<MessageStream, ProviderError>> + Send + 'a>> {
        Box::pin(async move {
            let client = Client::new();
            let model = if model_config.model.is_empty() {
                self.model.clone()
            } else {
                model_config.model.clone()
            };

            let mut openai_messages: Vec<OpenAiMessage> = vec![OpenAiMessage {
                role: "system".to_string(),
                content: Some(OpenAiContent::Text(system.to_string())),
                tool_calls: None,
                tool_call_id: None,
            }];

            for msg in messages {
                openai_messages.extend(openai_messages_from_goose(msg));
            }

            let openai_tools: Vec<OpenAiTool> = if tools.is_empty() {
                vec![]
            } else {
                tools
                    .iter()
                    .map(|t| OpenAiTool {
                        tool_type: "function".to_string(),
                        function: OpenAiFunction {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: t.parameters.clone(),
                        },
                    })
                    .collect()
            };

            let body = OpenAiChatRequest {
                model,
                messages: openai_messages,
                tools: if openai_tools.is_empty() {
                    None
                } else {
                    Some(openai_tools)
                },
                temperature: model_config.temperature,
                max_tokens: model_config.max_tokens,
                stream: Some(true),
            };

            let url = format!(
                "{}/chat/completions",
                normalize_openai_base_url(&self.base_url)
            );
            let request_retries = model_config.request_retries.min(MAX_REQUEST_RETRIES);
            let mut retries_done = 0;
            let response = loop {
                let result = client
                    .post(url.clone())
                    .header("User-Agent", PROVIDER_USER_AGENT)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await;

                match result {
                    Ok(response) if response.status().is_success() => break response,
                    Ok(response) => {
                        let status = response.status();
                        let text = response.text().await?;
                        if retryable_status(status) && retries_done < request_retries {
                            retries_done += 1;
                            sleep_before_retry(retries_done).await;
                            continue;
                        }
                        let total_attempts = retries_done + 1;
                        return Err(ProviderError::Message(format!(
                            "OpenAI API error {}{}: {}",
                            status,
                            retry_error_suffix(total_attempts),
                            text
                        )));
                    }
                    Err(err) => {
                        if retries_done < request_retries {
                            retries_done += 1;
                            sleep_before_retry(retries_done).await;
                            continue;
                        }
                        let total_attempts = retries_done + 1;
                        if total_attempts > 1 {
                            return Err(ProviderError::Message(format!(
                                "network error{}: {}",
                                retry_error_suffix(total_attempts),
                                err
                            )));
                        }
                        return Err(ProviderError::Network(err));
                    }
                }
            };

            let stream = response.bytes_stream();
            let usage = Arc::new(Mutex::new(ProviderUsage::default()));

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

                        let chunk: OpenAiChatChunk = match serde_json::from_str(data) {
                            Ok(c) => c,
                            Err(_) => continue,
                        };

                        for choice in chunk.choices {
                            if let Some(delta) = choice.delta {
                                if let Some(role) = delta.role {
                                    accumulated.role = Some(role);
                                }

                                if let Some(content) = delta.content {
                                    accumulated.content.push_str(&content);
                                    if let Some(msg) = accumulated.snapshot() {
                                        yield (Some(msg), ProviderUsage::default());
                                    }
                                }

                                if let Some(tool_calls) = delta.tool_calls {
                                    for tc in tool_calls {
                                        accumulated.add_tool_call(tc);
                                    }
                                }

                                if choice.finish_reason == Some("stop".to_string())
                                    || choice.finish_reason == Some("tool_calls".to_string())
                                {
                                    if let Some(msg) = accumulated.finish() {
                                        yield (Some(msg), ProviderUsage::default());
                                    }
                                    accumulated = AccumulatedMessage::default();
                                }
                            }

                            if let Some(ref u) = chunk.usage {
                                let mut guard = usage.lock().await;
                                guard.prompt_tokens = u.prompt_tokens;
                                guard.completion_tokens = u.completion_tokens;
                                guard.total_tokens = u.total_tokens;
                            }
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
    tool_calls: Vec<OpenAiToolCall>,
}

impl Default for AccumulatedMessage {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: None,
            content: String::new(),
            tool_calls: Vec::new(),
        }
    }
}

impl AccumulatedMessage {
    fn add_tool_call(&mut self, tc: OpenAiToolCallPartial) {
        if let Some(index) = tc.index {
            if let Some(existing) = self.tool_calls.get_mut(index) {
                if let Some(arguments) = tc.function.arguments {
                    existing.function.arguments.push_str(&arguments);
                }
                return;
            }
        }
        self.tool_calls.push(OpenAiToolCall {
            id: tc.id.unwrap_or_else(|| format!("call-{}", Uuid::new_v4())),
            tool_type: tc.r#type.unwrap_or_else(|| "function".to_string()),
            function: OpenAiFunctionCall {
                name: tc.function.name.unwrap_or_default(),
                arguments: tc.function.arguments.unwrap_or_default(),
            },
        });
    }

    fn snapshot(&self) -> Option<Message> {
        self.clone().finish()
    }

    fn finish(self) -> Option<Message> {
        let role = match self.role.as_deref() {
            Some("assistant") => Role::Assistant,
            Some("user") => Role::User,
            Some("system") => Role::System,
            _ => Role::Assistant,
        };

        let mut blocks: Vec<ContentBlock> = vec![];
        if !self.content.is_empty() {
            blocks.push(ContentBlock::Text { text: self.content });
        }

        for tc in self.tool_calls {
            blocks.push(ContentBlock::ToolRequest {
                id: tc.id,
                name: tc.function.name,
                arguments: serde_json::from_str(&tc.function.arguments)
                    .unwrap_or(serde_json::json!({})),
            });
        }

        if blocks.is_empty() {
            return None;
        }

        Some(Message {
            id: self.id,
            role,
            content: blocks,
            created_at: Utc::now(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct OpenAiChatChunk {
    id: Option<String>,
    object: String,
    created: u64,
    model: String,
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<ProviderUsage>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct OpenAiChoice {
    index: usize,
    delta: Option<OpenAiDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct OpenAiDelta {
    role: Option<String>,
    content: Option<String>,
    tool_calls: Option<Vec<OpenAiToolCallPartial>>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiToolCallPartial {
    index: Option<usize>,
    id: Option<String>,
    #[serde(rename = "type")]
    r#type: Option<String>,
    function: OpenAiFunctionPartial,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OpenAiToolCall {
    id: String,
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAiFunctionCall,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct OpenAiFunctionPartial {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<OpenAiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum OpenAiContent {
    Text(String),
    Parts(Vec<OpenAiContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiContentPart {
    #[serde(rename = "type")]
    part_type: String,
    text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAiFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

fn openai_messages_from_goose(msg: &Message) -> Vec<OpenAiMessage> {
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::System => "system",
        Role::Tool => "tool",
    };

    let mut tool_calls = vec![];
    let mut text_parts = vec![];
    let mut tool_responses = vec![];

    for block in &msg.content {
        match block {
            ContentBlock::Text { text } => {
                text_parts.push(text.clone());
            }
            ContentBlock::ToolRequest {
                id,
                name,
                arguments,
            } => {
                tool_calls.push(OpenAiToolCall {
                    id: id.clone(),
                    tool_type: "function".to_string(),
                    function: OpenAiFunctionCall {
                        name: name.clone(),
                        arguments: serde_json::to_string(arguments).unwrap_or_default(),
                    },
                });
            }
            ContentBlock::ToolResponse {
                id,
                content: resp_content,
                is_error: _,
            } => {
                tool_responses.push((id.clone(), resp_content.clone()));
            }
            ContentBlock::Thinking { text } => {
                let _ = text;
            }
        }
    }

    let mut messages = vec![];
    let content = if text_parts.is_empty() {
        None
    } else {
        Some(OpenAiContent::Text(text_parts.join("\n")))
    };
    if content.is_some() || !tool_calls.is_empty() {
        messages.push(OpenAiMessage {
            role: role.to_string(),
            content,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            tool_call_id: None,
        });
    }

    for (id, content) in tool_responses {
        messages.push(OpenAiMessage {
            role: "tool".to_string(),
            content: Some(OpenAiContent::Text(content)),
            tool_calls: None,
            tool_call_id: Some(id),
        });
    }

    messages
}

fn normalize_openai_base_url(value: &str) -> String {
    let mut base = extract_url_from_markdown(value)
        .unwrap_or(value)
        .trim()
        .trim_matches(['"', '\'', '`', '<', '>'])
        .trim()
        .to_string();

    loop {
        base = base.trim_end_matches(['/', '\\']).to_string();
        let lower = base.to_ascii_lowercase();
        let suffix = [
            "/chat/completions",
            "\\chat\\completions",
            "/responses",
            "\\responses",
        ]
        .into_iter()
        .find(|suffix| lower.ends_with(suffix));
        if let Some(suffix) = suffix {
            let new_len = base.len().saturating_sub(suffix.len());
            base.truncate(new_len);
            continue;
        }
        break;
    }

    base.trim_end_matches(['/', '\\']).to_string()
}

fn extract_url_from_markdown(value: &str) -> Option<&str> {
    let start = value.find("](")? + 2;
    let rest = &value[start..];
    let end = rest.find(')')?;
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_response_serializes_with_tool_call_id() {
        let message = Message::tool_response("call-1", "ok", false);

        let messages = openai_messages_from_goose(&message);

        assert_eq!(messages.len(), 1);
        let value = serde_json::to_value(&messages[0]).unwrap();
        assert_eq!(value["role"], "tool");
        assert_eq!(value["content"], "ok");
        assert_eq!(value["tool_call_id"], "call-1");
        assert!(value.get("tool_calls").is_none());
    }

    #[test]
    fn assistant_tool_request_serializes_as_tool_calls() {
        let message = Message {
            id: "msg-1".to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: "call-1".to_string(),
                name: "developer__list_files".to_string(),
                arguments: serde_json::json!({ "path": "." }),
            }],
            created_at: Utc::now(),
        };

        let messages = openai_messages_from_goose(&message);

        assert_eq!(messages.len(), 1);
        let value = serde_json::to_value(&messages[0]).unwrap();
        assert_eq!(value["role"], "assistant");
        assert_eq!(value["tool_calls"][0]["id"], "call-1");
        assert!(value.get("tool_call_id").is_none());
    }

    #[test]
    fn normalizes_base_url_endpoint_suffixes() {
        assert_eq!(
            normalize_openai_base_url(" https://api.fflink.top/v1/responses\\ "),
            "https://api.fflink.top/v1"
        );
        assert_eq!(
            normalize_openai_base_url("https://api.fflink.top/v1/chat/completions"),
            "https://api.fflink.top/v1"
        );
        assert_eq!(
            normalize_openai_base_url("[x](https://api.fflink.top/v1/responses\\)"),
            "https://api.fflink.top/v1"
        );
    }
}
