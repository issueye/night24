use std::future::Future;
use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::time::{sleep, Duration};

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
    pub request_retries: u8,
}

pub const MAX_REQUEST_RETRIES: u8 = 5;
pub const PROVIDER_USER_AGENT: &str = "Night24/0.1.0";

pub fn clamp_request_retries(value: Option<u8>) -> u8 {
    value.unwrap_or(0).min(MAX_REQUEST_RETRIES)
}

pub(crate) fn retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

pub(crate) fn retry_error_suffix(total_attempts: u8) -> String {
    if total_attempts > 1 {
        format!(" after {total_attempts} attempts")
    } else {
        String::new()
    }
}

pub(crate) async fn sleep_before_retry(retry_number: u8) {
    let delay_ms = match retry_number {
        0 | 1 => 300,
        2 => 700,
        3 => 1_500,
        4 => 3_000,
        _ => 5_000,
    };
    sleep(Duration::from_millis(delay_ms)).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_retries_are_clamped() {
        assert_eq!(clamp_request_retries(None), 0);
        assert_eq!(clamp_request_retries(Some(3)), 3);
        assert_eq!(clamp_request_retries(Some(8)), MAX_REQUEST_RETRIES);
    }

    #[test]
    fn retryable_statuses_are_limited_to_rate_limit_and_server_errors() {
        assert!(retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(retryable_status(StatusCode::BAD_GATEWAY));
        assert!(!retryable_status(StatusCode::BAD_REQUEST));
        assert!(!retryable_status(StatusCode::UNAUTHORIZED));
    }
}
