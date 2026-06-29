use std::future::Future;
use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{Message, Tool};

pub type MessageStream =
    Pin<Box<dyn Stream<Item = Result<(Option<Message>, ProviderUsage), ProviderError>> + Send>>;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("provider error: {0}")]
    Message(String),

    #[error("parse error: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ModelConfig {
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;

    fn stream<'a>(
        &'a self,
        model_config: &'a ModelConfig,
        system: &'a str,
        messages: &'a [Message],
        tools: &'a [Tool],
    ) -> Pin<Box<dyn Future<Output = Result<MessageStream, ProviderError>> + Send + 'a>>;
}

pub mod anthropic;
pub mod echo;
pub mod factory;
pub mod openai;
pub mod openai_responses;
pub mod registry;
pub mod tool_router;

pub use anthropic::AnthropicProvider;
pub use echo::EchoProvider;
pub use openai::OpenAIProvider;
pub use openai_responses::OpenAIResponsesProvider;
