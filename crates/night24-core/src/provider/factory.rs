use super::ModelConfig;
use super::Provider;
use crate::ollama_provider::OllamaProvider;
use std::sync::Arc;

pub trait ProviderFactory: Send + Sync {
    fn name(&self) -> &str;
    fn create(&self, model_config: ModelConfig) -> Arc<dyn Provider>;
}

pub struct EchoProviderFactory;

impl ProviderFactory for EchoProviderFactory {
    fn name(&self) -> &str {
        "echo"
    }

    fn create(&self, _model_config: ModelConfig) -> Arc<dyn Provider> {
        Arc::new(crate::provider::EchoProvider)
    }
}

pub struct OpenAIProviderFactory {
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
}

impl ProviderFactory for OpenAIProviderFactory {
    fn name(&self) -> &str {
        "openai"
    }

    fn create(&self, model_config: ModelConfig) -> Arc<dyn Provider> {
        let mut provider = crate::provider::OpenAIProvider::new(self.api_key.clone())
            .with_base_url(self.base_url.clone());
        if !model_config.model.is_empty() {
            provider = provider.with_model(model_config.model);
        } else if !self.default_model.is_empty() {
            provider = provider.with_model(self.default_model.clone());
        }
        Arc::new(provider)
    }
}

pub struct OpenAIResponsesProviderFactory {
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
}

impl ProviderFactory for OpenAIResponsesProviderFactory {
    fn name(&self) -> &str {
        "openai-responses"
    }

    fn create(&self, model_config: ModelConfig) -> Arc<dyn Provider> {
        let mut provider = crate::provider::OpenAIResponsesProvider::new(self.api_key.clone())
            .with_base_url(self.base_url.clone());
        if !model_config.model.is_empty() {
            provider = provider.with_model(model_config.model);
        } else if !self.default_model.is_empty() {
            provider = provider.with_model(self.default_model.clone());
        }
        Arc::new(provider)
    }
}

pub struct AnthropicProviderFactory {
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
}

impl ProviderFactory for AnthropicProviderFactory {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn create(&self, model_config: ModelConfig) -> Arc<dyn Provider> {
        let mut provider = crate::provider::AnthropicProvider::new(self.api_key.clone())
            .with_base_url(self.base_url.clone());
        if !model_config.model.is_empty() {
            provider = provider.with_model(model_config.model);
        } else if !self.default_model.is_empty() {
            provider = provider.with_model(self.default_model.clone());
        }
        Arc::new(provider)
    }
}

pub struct StepFunProviderFactory {
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
}

impl ProviderFactory for StepFunProviderFactory {
    fn name(&self) -> &str {
        "stepfun"
    }

    fn create(&self, model_config: ModelConfig) -> Arc<dyn Provider> {
        let mut provider = crate::provider::OpenAIProvider::new(self.api_key.clone())
            .with_base_url(self.base_url.clone());
        if !model_config.model.is_empty() {
            provider = provider.with_model(model_config.model);
        } else if !self.default_model.is_empty() {
            provider = provider.with_model(self.default_model.clone());
        }
        Arc::new(provider)
    }
}

pub struct OllamaProviderFactory {
    pub base_url: String,
    pub default_model: String,
}

impl ProviderFactory for OllamaProviderFactory {
    fn name(&self) -> &str {
        "ollama"
    }

    fn create(&self, model_config: ModelConfig) -> Arc<dyn Provider> {
        let mut provider = OllamaProvider::new(self.base_url.clone());
        if !model_config.model.is_empty() {
            provider = provider.with_model(model_config.model);
        } else if !self.default_model.is_empty() {
            provider = provider.with_model(self.default_model.clone());
        }
        Arc::new(provider)
    }
}
