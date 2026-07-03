use std::sync::Arc;

use night24_core::provider::{AnthropicProvider, EchoProvider, OpenAIProvider, Provider};
use night24_core::OllamaProvider;
use night24_protocol::ProviderConfig;

pub(super) fn effective_provider(
    config: &ProviderConfig,
    default_provider: &str,
) -> ProviderConfig {
    let mut config = config.clone();
    if config.provider.trim().is_empty() {
        config.provider = default_provider.to_string();
    }
    config.provider = config.provider.trim().to_ascii_lowercase();
    config.model = config.model.trim().to_string();
    config.base_url = config
        .base_url
        .and_then(|value| non_empty(&value).map(str::to_string));
    config.api_key = config
        .api_key
        .and_then(|value| non_empty(&value).map(str::to_string));
    config.api_key_ref = config
        .api_key_ref
        .and_then(|value| non_empty(&value).map(str::to_string));
    config
}

pub(super) fn create_provider(config: &ProviderConfig) -> anyhow::Result<Arc<dyn Provider>> {
    match config.provider.as_str() {
        "echo" => Ok(Arc::new(EchoProvider)),
        "openai" | "openai-responses" => {
            let api_key = resolve_api_key(config, "OPENAI_API_KEY")?;
            let base_url = resolve_base_url(config, "OPENAI_BASE_URL", "https://api.openai.com/v1");
            Ok(Arc::new(
                OpenAIProvider::new(api_key)
                    .with_base_url(base_url)
                    .with_model(effective_model(config)),
            ))
        }
        "stepfun" => {
            let api_key = resolve_api_key(config, "STEPFUN_API_KEY")?;
            let base_url = resolve_base_url(
                config,
                "STEPFUN_BASE_URL",
                "https://api.stepfun.com/step_plan/v1",
            );
            Ok(Arc::new(
                OpenAIProvider::new(api_key)
                    .with_base_url(base_url)
                    .with_model(effective_model(config)),
            ))
        }
        "anthropic" => {
            let api_key = resolve_api_key(config, "ANTHROPIC_API_KEY")?;
            let base_url =
                resolve_base_url(config, "ANTHROPIC_BASE_URL", "https://api.anthropic.com/v1");
            Ok(Arc::new(
                AnthropicProvider::new(api_key)
                    .with_base_url(base_url)
                    .with_model(effective_model(config)),
            ))
        }
        "ollama" => {
            let base_url = resolve_base_url(config, "OLLAMA_BASE_URL", "http://localhost:11434");
            Ok(Arc::new(
                OllamaProvider::new(base_url).with_model(effective_model(config)),
            ))
        }
        other => anyhow::bail!("unknown provider: {other}"),
    }
}

pub(super) fn effective_model(config: &ProviderConfig) -> String {
    if !config.model.trim().is_empty() {
        return config.model.trim().to_string();
    }
    match config.provider.as_str() {
        "openai" | "openai-responses" => std::env::var("OPENAI_MODEL")
            .ok()
            .and_then(|value| non_empty(&value).map(str::to_string))
            .unwrap_or_else(|| "gpt-4o-mini".to_string()),
        "stepfun" => std::env::var("STEPFUN_MODEL")
            .ok()
            .and_then(|value| non_empty(&value).map(str::to_string))
            .unwrap_or_else(|| "step-3.7-flash".to_string()),
        "anthropic" => std::env::var("ANTHROPIC_MODEL")
            .ok()
            .and_then(|value| non_empty(&value).map(str::to_string))
            .unwrap_or_else(|| "claude-3-5-sonnet-latest".to_string()),
        "ollama" => std::env::var("OLLAMA_MODEL")
            .ok()
            .and_then(|value| non_empty(&value).map(str::to_string))
            .unwrap_or_else(|| "llama3.2".to_string()),
        _ => "echo-v1".to_string(),
    }
}

fn resolve_api_key(config: &ProviderConfig, env_name: &str) -> anyhow::Result<String> {
    if let Some(key) = config.api_key.as_deref().and_then(non_empty) {
        return Ok(key.to_string());
    }

    if let Some(key_ref) = config.api_key_ref.as_deref().and_then(non_empty) {
        let key = std::env::var(key_ref)
            .map_err(|_| anyhow::anyhow!("API key env var is not set: {key_ref}"))?;
        if let Some(key) = non_empty(&key) {
            return Ok(key.to_string());
        }
        anyhow::bail!("API key env var is empty: {key_ref}");
    }

    let key = std::env::var(env_name)
        .map_err(|_| anyhow::anyhow!("api_key is required for {} provider", config.provider))?;
    non_empty(&key)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("api_key is required for {} provider", config.provider))
}

fn resolve_base_url(config: &ProviderConfig, env_name: &str, default_value: &str) -> String {
    config
        .base_url
        .as_deref()
        .and_then(non_empty)
        .map(str::to_string)
        .or_else(|| {
            std::env::var(env_name)
                .ok()
                .and_then(|value| non_empty(&value).map(str::to_string))
        })
        .unwrap_or_else(|| default_value.to_string())
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
