use std::collections::HashMap;
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
use crate::provider::{
    retry_error_suffix, retryable_status, sleep_before_retry, MessageStream, ModelConfig, Provider,
    ProviderError, ProviderUsage, MAX_REQUEST_RETRIES, PROVIDER_USER_AGENT,
};

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
        self.base_url = normalize_openai_base_url(&base_url.into());
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

            let input_items = responses_input_items_from_messages(messages);

            let body = ResponsesRequest {
                model,
                instructions: if system.is_empty() {
                    None
                } else {
                    Some(system.to_string())
                },
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

            let url = format!("{}/responses", normalize_openai_base_url(&self.base_url));
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
                            "OpenAI Responses API error {}{}: {}{}",
                            status,
                            retry_error_suffix(total_attempts),
                            text,
                            responses_endpoint_hint(&self.base_url)
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
                            Some("response.created") => {
                                if let Some(response) = event.response {
                                    for item in response.output {
                                        accumulated.apply_item(item);
                                    }
                                }
                            }
                            Some("response.output_item.added") => {
                                if let Some(item) = event.item {
                                    accumulated.apply_item(item);
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
                                    accumulated.push_tool_call_arguments(
                                        event.item_id.as_deref(),
                                        event.call_id.as_deref(),
                                        &delta,
                                    );
                                }
                            }
                            Some("response.output_text.done") => {
                                if let Some(text) = event.text {
                                    accumulated.append_text(&text);
                                    if let Some(msg) = accumulated.snapshot() {
                                        yield (Some(msg), ProviderUsage::default());
                                    }
                                }
                            }
                            Some("response.output_item.done") => {
                                if let Some(item) = event.item {
                                    if item.r#type == "function_call" {
                                        let arguments = accumulated.arguments_for_item(&item);
                                        if let Some(idx) = accumulated.tool_calls.iter().position(|tc| tc.id == item.id.clone().unwrap_or_default() || tc.call_id == item.call_id.clone().unwrap_or_default()) {
                                            if let Some(tc) = accumulated.tool_calls.get_mut(idx) {
                                                if !arguments.is_empty() {
                                                    tc.arguments = arguments;
                                                }
                                            }
                                        } else {
                                            let mut item = item;
                                            if !arguments.is_empty() {
                                                item.arguments = Some(arguments);
                                            }
                                            accumulated.apply_item(item);
                                        }
                                    } else {
                                        accumulated.apply_item(item);
                                        if let Some(msg) = accumulated.snapshot() {
                                            yield (Some(msg), ProviderUsage::default());
                                        }
                                    }
                                }
                            }
                            Some("response.completed") => {
                                if let Some(response) = event.response {
                                    for item in response.output {
                                        accumulated.apply_item(item);
                                    }
                                }
                                if let Some(msg) = accumulated.finish() {
                                    yield (Some(msg), ProviderUsage::default());
                                }
                                accumulated = AccumulatedMessage::default();
                            }
                            _ => {}
                        }
                    }
                }
                if let Some(msg) = accumulated.finish() {
                    yield (Some(msg), ProviderUsage::default());
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
    tool_call_arguments: HashMap<String, String>,
}

impl Default for AccumulatedMessage {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: None,
            content: String::new(),
            tool_calls: Vec::new(),
            tool_call_arguments: HashMap::new(),
        }
    }
}

impl AccumulatedMessage {
    fn push_tool_call_arguments(
        &mut self,
        item_id: Option<&str>,
        call_id: Option<&str>,
        delta: &str,
    ) {
        let key = item_id
            .filter(|value| !value.is_empty())
            .or(call_id.filter(|value| !value.is_empty()))
            .unwrap_or("__current__");
        self.tool_call_arguments
            .entry(key.to_string())
            .or_default()
            .push_str(delta);
    }

    fn arguments_for_item(&mut self, item: &ResponsesItem) -> String {
        if let Some(arguments) = item
            .arguments
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return arguments.to_string();
        }

        for key in [
            item.id.as_deref(),
            item.call_id.as_deref(),
            Some("__current__"),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(arguments) = self.tool_call_arguments.remove(key) {
                if !arguments.trim().is_empty() {
                    return arguments;
                }
            }
        }

        String::new()
    }

    fn append_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.content == text || self.content.ends_with(text) {
            return;
        }
        if text.starts_with(&self.content) {
            self.content = text.to_string();
        } else {
            self.content.push_str(text);
        }
    }

    fn apply_item(&mut self, item: ResponsesItem) {
        match item.r#type.as_str() {
            "message" => {
                self.role = Some(item.role.unwrap_or_else(|| "assistant".to_string()));
                for content in item.content {
                    if matches!(
                        content.r#type.as_deref(),
                        Some("output_text" | "text" | "input_text") | None
                    ) {
                        self.append_text(&content.text.unwrap_or_default());
                    }
                }
            }
            "function_call" => {
                let id = item.id.unwrap_or_default();
                let call_id = item.call_id.unwrap_or_else(|| id.clone());
                let arguments = item.arguments.unwrap_or_default();
                if let Some(existing) = self.tool_calls.iter_mut().find(|tc| {
                    (!id.is_empty() && tc.id == id)
                        || (!call_id.is_empty() && tc.call_id == call_id)
                }) {
                    existing.name = item.name.unwrap_or_else(|| existing.name.clone());
                    if !arguments.is_empty() {
                        existing.arguments = arguments;
                    }
                } else {
                    self.tool_calls.push(ResponsesToolCall {
                        id,
                        name: item.name.unwrap_or_default(),
                        arguments,
                        call_id,
                    });
                }
            }
            _ => {}
        }
    }

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
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
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
#[serde(tag = "type")]
enum ResponsesInputItem {
    #[serde(rename = "message")]
    Message { role: String, content: String },
    #[serde(rename = "function_call")]
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    #[serde(rename = "function_call_output")]
    FunctionCallOutput { call_id: String, output: String },
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
    item_id: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    item: Option<ResponsesItem>,
    #[serde(default)]
    response: Option<ResponsesResponse>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResponsesItem {
    #[serde(rename = "type")]
    r#type: String,
    id: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: Vec<ResponsesContent>,
    name: Option<String>,
    arguments: Option<String>,
    call_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResponsesContent {
    #[serde(rename = "type")]
    r#type: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResponsesResponse {
    #[serde(default)]
    output: Vec<ResponsesItem>,
}

#[derive(Debug, Clone)]
struct ResponsesToolCall {
    id: String,
    call_id: String,
    name: String,
    arguments: String,
}

fn responses_input_items_from_goose(
    msg: &Message,
    completed_call_ids: &std::collections::HashSet<String>,
    known_call_ids: &mut std::collections::HashSet<String>,
) -> Vec<ResponsesInputItem> {
    let mut text_parts = vec![];
    let mut items = vec![];

    for block in &msg.content {
        match block {
            ContentBlock::Text { text } => text_parts.push(text.clone()),
            ContentBlock::ToolRequest {
                id,
                name,
                arguments,
            } => {
                if !completed_call_ids.contains(id) {
                    continue;
                }
                known_call_ids.insert(id.clone());
                items.push(ResponsesInputItem::FunctionCall {
                    call_id: id.clone(),
                    name: name.clone(),
                    arguments: serde_json::to_string(arguments).unwrap_or_default(),
                });
            }
            ContentBlock::ToolResponse {
                id,
                content,
                is_error,
            } => {
                if !known_call_ids.contains(id) {
                    continue;
                }
                let output = if *is_error {
                    format!("error: {}", content)
                } else {
                    content.clone()
                };
                items.push(ResponsesInputItem::FunctionCallOutput {
                    call_id: id.clone(),
                    output,
                });
            }
            ContentBlock::Thinking { text } => {
                let _ = text;
            }
        }
    }

    if !text_parts.is_empty() {
        let role = match msg.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "user",
        };
        items.insert(
            0,
            ResponsesInputItem::Message {
                role: role.to_string(),
                content: text_parts.join("\n"),
            },
        );
    }

    items
}

fn responses_input_items_from_messages(messages: &[Message]) -> Vec<ResponsesInputItem> {
    let completed_call_ids = tool_response_call_ids(messages);
    let mut known_call_ids = std::collections::HashSet::new();
    let mut input_items = vec![];
    for msg in messages {
        input_items.extend(responses_input_items_from_goose(
            msg,
            &completed_call_ids,
            &mut known_call_ids,
        ));
    }
    input_items
}

fn tool_response_call_ids(messages: &[Message]) -> std::collections::HashSet<String> {
    messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            ContentBlock::ToolResponse { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

fn responses_endpoint_hint(base_url: &str) -> &'static str {
    if base_url.contains("api.openai.com") {
        ""
    } else {
        " Hint: the OpenAI Responses protocol requires a base URL that supports POST /responses. If this endpoint only supports Chat Completions-compatible APIs, switch the provider to openai-chat."
    }
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

    base
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
    fn tool_response_serializes_as_function_call_output() {
        let message = Message::tool_response("call-1", "ok", false);
        let completed_call_ids = std::collections::HashSet::from(["call-1".to_string()]);
        let mut known_call_ids = std::collections::HashSet::from(["call-1".to_string()]);

        let items =
            responses_input_items_from_goose(&message, &completed_call_ids, &mut known_call_ids);

        assert_eq!(items.len(), 1);
        let value = serde_json::to_value(&items[0]).unwrap();
        assert_eq!(value["type"], "function_call_output");
        assert_eq!(value["call_id"], "call-1");
        assert_eq!(value["output"], "ok");
        assert!(value.get("role").is_none());
    }

    #[test]
    fn assistant_tool_request_serializes_as_function_call_when_output_exists() {
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
        let completed_call_ids = std::collections::HashSet::from(["call-1".to_string()]);
        let mut known_call_ids = std::collections::HashSet::new();

        let items =
            responses_input_items_from_goose(&message, &completed_call_ids, &mut known_call_ids);

        assert_eq!(items.len(), 1);
        let value = serde_json::to_value(&items[0]).unwrap();
        assert_eq!(value["type"], "function_call");
        assert_eq!(value["call_id"], "call-1");
        assert_eq!(value["name"], "developer__list_files");
        assert_eq!(value["arguments"], "{\"path\":\".\"}");
        assert!(value.get("role").is_none());
    }

    #[test]
    fn tool_role_text_does_not_serialize_as_tool_role() {
        let message = Message {
            id: "msg-1".to_string(),
            role: Role::Tool,
            content: vec![ContentBlock::Text {
                text: "legacy tool text".to_string(),
            }],
            created_at: Utc::now(),
        };
        let completed_call_ids = std::collections::HashSet::new();
        let mut known_call_ids = std::collections::HashSet::new();

        let items =
            responses_input_items_from_goose(&message, &completed_call_ids, &mut known_call_ids);

        assert_eq!(items.len(), 1);
        let value = serde_json::to_value(&items[0]).unwrap();
        assert_eq!(value["type"], "message");
        assert_eq!(value["role"], "user");
    }

    #[test]
    fn orphan_tool_response_is_not_serialized_without_matching_function_call() {
        let message = Message::tool_response("missing-call", "orphan output", false);
        let completed_call_ids = std::collections::HashSet::from(["missing-call".to_string()]);
        let mut known_call_ids = std::collections::HashSet::new();

        let items =
            responses_input_items_from_goose(&message, &completed_call_ids, &mut known_call_ids);

        assert!(items.is_empty());
    }

    #[test]
    fn orphan_tool_request_is_not_serialized_without_matching_output() {
        let message = Message {
            id: "msg-1".to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: "call-orphan".to_string(),
                name: "developer__read_file".to_string(),
                arguments: serde_json::json!({ "path": "README.md" }),
            }],
            created_at: Utc::now(),
        };

        let items = responses_input_items_from_messages(&[message]);

        assert!(items.is_empty());
    }

    #[test]
    fn paired_tool_request_and_response_are_serialized_in_order() {
        let request = Message {
            id: "msg-request".to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: "call-1".to_string(),
                name: "developer__read_file".to_string(),
                arguments: serde_json::json!({ "path": "README.md" }),
            }],
            created_at: Utc::now(),
        };
        let response = Message::tool_response("call-1", "file contents", false);

        let items = responses_input_items_from_messages(&[request, response]);

        assert_eq!(items.len(), 2);
        let first = serde_json::to_value(&items[0]).unwrap();
        let second = serde_json::to_value(&items[1]).unwrap();
        assert_eq!(first["type"], "function_call");
        assert_eq!(first["call_id"], "call-1");
        assert_eq!(second["type"], "function_call_output");
        assert_eq!(second["call_id"], "call-1");
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

    #[test]
    fn response_created_output_preserves_function_call_for_http_tool_followup() {
        let event: ResponsesEvent = serde_json::from_value(serde_json::json!({
            "type": "response.created",
            "response": {
                "output": [
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            { "type": "output_text", "text": "我会先查看目录。" }
                        ]
                    },
                    {
                        "type": "function_call",
                        "call_id": "call-1",
                        "name": "developer__list_files",
                        "arguments": "{}"
                    }
                ]
            }
        }))
        .unwrap();
        let mut accumulated = AccumulatedMessage::default();

        for item in event.response.unwrap().output {
            accumulated.apply_item(item);
        }
        let message = accumulated
            .finish()
            .expect("message should contain a tool request");

        assert_eq!(message.role, Role::Assistant);
        assert!(matches!(
            message.content[0],
            ContentBlock::Text { ref text } if text == "我会先查看目录。"
        ));
        assert!(matches!(
            message.content[1],
            ContentBlock::ToolRequest { ref id, ref name, .. }
                if id == "call-1" && name == "developer__list_files"
        ));
    }

    #[test]
    fn function_call_argument_deltas_are_kept_per_tool_call() {
        let mut accumulated = AccumulatedMessage::default();
        accumulated.apply_item(ResponsesItem {
            r#type: "function_call".to_string(),
            id: Some("item-1".to_string()),
            role: None,
            content: vec![],
            name: Some("developer__write_file".to_string()),
            arguments: None,
            call_id: Some("call-1".to_string()),
        });
        accumulated.apply_item(ResponsesItem {
            r#type: "function_call".to_string(),
            id: Some("item-2".to_string()),
            role: None,
            content: vec![],
            name: Some("developer__read_file".to_string()),
            arguments: None,
            call_id: Some("call-2".to_string()),
        });

        accumulated.push_tool_call_arguments(Some("item-1"), Some("call-1"), "{\"path\":\"a.txt\"");
        accumulated.push_tool_call_arguments(
            Some("item-1"),
            Some("call-1"),
            ",\"content\":\"hello\"}",
        );
        accumulated.push_tool_call_arguments(
            Some("item-2"),
            Some("call-2"),
            "{\"path\":\"b.txt\"}",
        );

        let mut done_write = ResponsesItem {
            r#type: "function_call".to_string(),
            id: Some("item-1".to_string()),
            role: None,
            content: vec![],
            name: Some("developer__write_file".to_string()),
            arguments: None,
            call_id: Some("call-1".to_string()),
        };
        done_write.arguments = Some(accumulated.arguments_for_item(&done_write));
        accumulated.apply_item(done_write);

        let mut done_read = ResponsesItem {
            r#type: "function_call".to_string(),
            id: Some("item-2".to_string()),
            role: None,
            content: vec![],
            name: Some("developer__read_file".to_string()),
            arguments: None,
            call_id: Some("call-2".to_string()),
        };
        done_read.arguments = Some(accumulated.arguments_for_item(&done_read));
        accumulated.apply_item(done_read);

        let message = accumulated.finish().expect("tool calls should finish");
        assert!(matches!(
            &message.content[0],
            ContentBlock::ToolRequest { id, name, arguments }
                if id == "call-1"
                    && name == "developer__write_file"
                    && arguments["path"] == "a.txt"
                    && arguments["content"] == "hello"
        ));
        assert!(matches!(
            &message.content[1],
            ContentBlock::ToolRequest { id, name, arguments }
                if id == "call-2"
                    && name == "developer__read_file"
                    && arguments["path"] == "b.txt"
        ));
    }
}
