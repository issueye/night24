use std::future::Future;
use std::pin::Pin;

use async_trait::async_trait;
use async_stream::try_stream;
use chrono::Utc;
use serde_json;
use uuid::Uuid;

use crate::model::{ContentBlock, Message, Role};
use crate::provider::{
    MessageStream, ModelConfig, Provider, ProviderError, ProviderUsage,
};

#[derive(Debug, Default)]
pub struct EchoProvider;

#[async_trait]
impl Provider for EchoProvider {
    fn name(&self) -> &str {
        "echo"
    }

    fn stream<'a>(
        &'a self,
        _model_config: &'a ModelConfig,
        _system: &'a str,
        messages: &'a [Message],
        _tools: &'a [crate::model::Tool],
    ) -> Pin<Box<dyn Future<Output = Result<MessageStream, ProviderError>> + Send + 'a>> {
        Box::pin(async move {
            let last = messages.last().cloned();
            let stream = try_stream! {
                if let Some(msg) = last {
                    let text = msg
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            ContentBlock::Text { text } => Some(text.clone()),
                            ContentBlock::ToolRequest { id: _, name: _, arguments } => {
                                let args_str = serde_json::to_string(arguments).unwrap_or_default();
                                Some(format!("[tool request: {}]", args_str))
                            }
                            ContentBlock::ToolResponse { id: _, content, is_error: _ } => Some(content.clone()),
                            ContentBlock::Thinking { text } => Some(format!("[thinking] {}", text)),
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    if text.starts_with("tool:") {
                        let tool_input = text.trim_start_matches("tool:").trim().to_string();
                        let routed = crate::provider::tool_router::route_tool_input(&tool_input).await;
                        if let Some(tool_msg) = routed {
                            yield (Some(tool_msg), ProviderUsage::default());
                        } else {
                            let response = Message {
                                id: Uuid::new_v4().to_string(),
                                role: Role::Assistant,
                                content: vec![ContentBlock::Text { text: format!("[echo] {}", tool_input) }],
                                created_at: Utc::now(),
                            };
                            yield (Some(response), ProviderUsage::default());
                        }
                    } else {
                        let response = Message {
                            id: Uuid::new_v4().to_string(),
                            role: Role::Assistant,
                            content: vec![ContentBlock::Text { text: format!("[echo] {}", text) }],
                            created_at: Utc::now(),
                        };
                        yield (Some(response), ProviderUsage::default());
                    }
                }
            };

            Ok(Box::pin(stream) as MessageStream)
        })
    }
}
