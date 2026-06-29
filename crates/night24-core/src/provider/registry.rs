use super::factory::{
    AnthropicProviderFactory, EchoProviderFactory, OpenAIProviderFactory, OpenAIResponsesProviderFactory,
    StepFunProviderFactory, OllamaProviderFactory, ProviderFactory,
};
use super::{ModelConfig, Provider};
use std::collections::HashMap;
use std::sync::Arc;

pub struct ProviderRegistry {
    factories: HashMap<String, Box<dyn ProviderFactory + 'static>>,
    default: String,
}

impl ProviderRegistry {
    pub fn new(default: impl Into<String>) -> Self {
        Self {
            factories: HashMap::new(),
            default: default.into(),
        }
    }

    pub fn register(mut self, factory: Box<dyn ProviderFactory + 'static>) -> Self {
        let name = factory.name().to_string();
        self.factories.insert(name, factory);
        self
    }

    pub fn with_echo(mut self) -> Self {
        self.factories
            .insert("echo".to_string(), Box::new(EchoProviderFactory));
        self
    }

    pub fn with_openai(mut self, api_key: impl Into<String>, base_url: impl Into<String>, default_model: impl Into<String>) -> Self {
        self.factories.insert(
            "openai".to_string(),
            Box::new(OpenAIProviderFactory {
                api_key: api_key.into(),
                base_url: base_url.into(),
                default_model: default_model.into(),
            }),
        );
        self
    }

    pub fn with_openai_responses(mut self, api_key: impl Into<String>, base_url: impl Into<String>, default_model: impl Into<String>) -> Self {
        self.factories.insert(
            "openai-responses".to_string(),
            Box::new(OpenAIResponsesProviderFactory {
                api_key: api_key.into(),
                base_url: base_url.into(),
                default_model: default_model.into(),
            }),
        );
        self
    }

    pub fn with_anthropic(mut self, api_key: impl Into<String>, base_url: impl Into<String>, default_model: impl Into<String>) -> Self {
        self.factories.insert(
            "anthropic".to_string(),
            Box::new(AnthropicProviderFactory {
                api_key: api_key.into(),
                base_url: base_url.into(),
                default_model: default_model.into(),
            }),
        );
        self
    }

    pub fn with_stepfun(mut self, api_key: impl Into<String>, base_url: impl Into<String>, default_model: impl Into<String>) -> Self {
        self.factories.insert(
            "stepfun".to_string(),
            Box::new(StepFunProviderFactory {
                api_key: api_key.into(),
                base_url: base_url.into(),
                default_model: default_model.into(),
            }),
        );
        self
    }

    pub fn with_ollama(mut self, base_url: impl Into<String>, default_model: impl Into<String>) -> Self {
        self.factories.insert(
            "ollama".to_string(),
            Box::new(OllamaProviderFactory {
                base_url: base_url.into(),
                default_model: default_model.into(),
            }),
        );
        self
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Provider>> {
        self.factories
            .get(name)
            .or_else(|| if name == self.default { None } else { None })
            .map(|f| f.create(ModelConfig::default()))
    }

    pub fn get_with_model(&self, name: &str, model: impl Into<String>) -> Option<Arc<dyn Provider>> {
        let model_config = ModelConfig {
            model: model.into(),
            ..Default::default()
        };
        self.factories
            .get(name)
            .or_else(|| if name == self.default { None } else { None })
            .map(|f| f.create(model_config))
    }

    pub fn create(&self, name: &str) -> Arc<dyn Provider> {
        self.get(name).unwrap_or_else(|| {
            self.factories
                .get(&self.default)
                .map(|f| f.create(ModelConfig::default()))
                .expect("default provider factory is not registered")
        })
    }

    pub fn create_with_model(&self, name: &str, model: impl Into<String>) -> Arc<dyn Provider> {
        let model_str = model.into();
        let model_config = ModelConfig {
            model: model_str.clone(),
            ..Default::default()
        };
        self.get_with_model(name, model_str).unwrap_or_else(|| {
            self.factories
                .get(&self.default)
                .map(|f| f.create(model_config))
                .expect("default provider factory is not registered")
        })
    }

    pub fn names(&self) -> Vec<String> {
        self.factories.keys().cloned().collect()
    }
}
