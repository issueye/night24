use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use night24_core::agent::{Agent, AgentConfig};
use night24_core::model::{ContentBlock, Message, Role};
use night24_core::provider::{
    AnthropicProvider, EchoProvider, ModelConfig, OpenAIProvider, Provider,
};
use night24_core::OllamaProvider;
use night24_core::session::{Session, SessionType};
use night24_protocol::{
    AcceptedResult, AgentEvent, AgentEventKind, AgentToolsParams, AgentToolsResult, CancelParams,
    Capability, FinishStatus, InitializeParams, InitializeResult, JsonRpcError, JsonRpcId,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, PeerInfo, PingParams, PingResult,
    ProviderConfig, ReplyAccepted, ReplyParams, ShutdownParams, PROTOCOL_VERSION,
};
use serde::de::DeserializeOwned;
use tokio::sync::mpsc::UnboundedSender;

const SERVER_NAME: &str = "night24-agent-core";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoreState {
    Spawned,
    Initialized,
    Draining,
}

pub struct AgentCore {
    state: CoreState,
    exit_after_flush: bool,
    default_provider: String,
    output: Option<UnboundedSender<String>>,
}

impl Default for AgentCore {
    fn default() -> Self {
        Self {
            state: CoreState::Spawned,
            exit_after_flush: false,
            default_provider: "echo".to_string(),
            output: None,
        }
    }
}

impl AgentCore {
    pub fn with_output(output: UnboundedSender<String>) -> Self {
        Self {
            output: Some(output),
            ..Self::default()
        }
    }

    pub fn should_exit(&self) -> bool {
        self.exit_after_flush
    }

    pub async fn handle_line(&mut self, line: &str) -> Vec<String> {
        match serde_json::from_str::<JsonRpcRequest>(line) {
            Ok(request) => self.handle_request(request).await,
            Err(err) => {
                eprintln!("invalid JSON-RPC request: {err}");
                vec![serialize_response(JsonRpcResponse::error(
                    JsonRpcId::from("rpc-parse-error"),
                    JsonRpcError::parse_error("parse error"),
                ))]
            }
        }
    }

    async fn handle_request(&mut self, request: JsonRpcRequest) -> Vec<String> {
        if request.jsonrpc != "2.0" {
            return vec![self.error_response(
                request.id,
                JsonRpcError::invalid_request("jsonrpc must be \"2.0\""),
            )];
        }

        let id = request.id;
        match request.method.as_str() {
            "core.initialize" => self.initialize(id, request.params),
            "core.ping" => self.ping(id, request.params),
            "core.shutdown" => self.shutdown(id, request.params),
            "agent.tools" => {
                if !self.is_initialized() {
                    return vec![self.error_response(id, JsonRpcError::core_not_initialized())];
                }
                self.agent_tools(id, request.params)
            }
            "agent.reply" => {
                if !self.is_initialized() {
                    return vec![self.error_response(id, JsonRpcError::core_not_initialized())];
                }
                self.agent_reply(id, request.params).await
            }
            "agent.cancel" => {
                if !self.is_initialized() {
                    return vec![self.error_response(id, JsonRpcError::core_not_initialized())];
                }
                self.agent_cancel(id, request.params)
            }
            "permission.resolve" => {
                vec![self.success_response(id, AcceptedResult { accepted: true })]
            }
            method => vec![self.error_response(id, JsonRpcError::method_not_found(method))],
        }
    }

    fn initialize(&mut self, id: JsonRpcId, params: Option<serde_json::Value>) -> Vec<String> {
        if self.state != CoreState::Spawned {
            return vec![self.error_response(
                id,
                JsonRpcError::protocol_violation("core is already initialized"),
            )];
        }

        let params = match decode_params::<InitializeParams>(params) {
            Ok(params) => params,
            Err(err) => return vec![self.error_response(id, err)],
        };

        if params.protocol_version != PROTOCOL_VERSION {
            return vec![self.error_response(
                id,
                JsonRpcError::protocol_violation(format!(
                    "unsupported protocol_version: {}",
                    params.protocol_version
                )),
            )];
        }

        if let Some(default_provider) = params.environment.default_provider {
            if !default_provider.trim().is_empty() {
                self.default_provider = default_provider;
            }
        }

        self.state = CoreState::Initialized;
        let result = InitializeResult {
            protocol_version: PROTOCOL_VERSION.to_string(),
            server: PeerInfo::new(SERVER_NAME, env!("CARGO_PKG_VERSION")),
            capabilities: core_capabilities(),
        };

        vec![self.success_response(id, result)]
    }

    fn ping(&self, id: JsonRpcId, params: Option<serde_json::Value>) -> Vec<String> {
        let params = match decode_optional_params::<PingParams>(params) {
            Ok(params) => params,
            Err(err) => return vec![self.error_response(id, err)],
        };

        vec![self.success_response(
            id,
            PingResult {
                nonce: params.nonce,
                status: "ok".to_string(),
            },
        )]
    }

    fn shutdown(&mut self, id: JsonRpcId, params: Option<serde_json::Value>) -> Vec<String> {
        if let Err(err) = decode_optional_params::<ShutdownParams>(params) {
            return vec![self.error_response(id, err)];
        }

        self.state = CoreState::Draining;
        self.exit_after_flush = true;
        vec![self.success_response(id, AcceptedResult { accepted: true })]
    }

    fn agent_tools(&self, id: JsonRpcId, params: Option<serde_json::Value>) -> Vec<String> {
        if let Err(err) = decode_optional_params::<AgentToolsParams>(params) {
            return vec![self.error_response(id, err)];
        }

        vec![self.success_response(
            id,
            AgentToolsResult {
                tools: night24_core::tool_executor::builtin_tools(),
            },
        )]
    }

    async fn agent_reply(&self, id: JsonRpcId, params: Option<serde_json::Value>) -> Vec<String> {
        let params = match decode_params::<ReplyParams>(params) {
            Ok(params) => params,
            Err(err) => return vec![self.error_response(id, err)],
        };

        let accepted = self.success_response(
            id,
            ReplyAccepted {
                accepted: true,
                run_id: params.run_id.clone(),
            },
        );

        if let Some(output) = self.output.clone() {
            let default_provider = self.default_provider.clone();
            tokio::spawn(async move {
                for message in reply_events(params, default_provider).await {
                    let _ = output.send(message);
                }
            });
            return vec![accepted];
        }

        let mut output = vec![accepted];
        output.extend(reply_events(params, self.default_provider.clone()).await);
        output
    }

    fn agent_cancel(&self, id: JsonRpcId, params: Option<serde_json::Value>) -> Vec<String> {
        if let Err(err) = decode_params::<CancelParams>(params) {
            return vec![self.error_response(id, err)];
        }

        vec![self.success_response(id, AcceptedResult { accepted: true })]
    }

    async fn run_agent(params: &ReplyParams, default_provider: &str) -> anyhow::Result<Vec<Message>> {
        let provider_config = effective_provider(&params.provider, default_provider);
        let model = effective_model(&provider_config);
        let provider = create_provider(&provider_config)?;

        let config = AgentConfig {
            model_config: ModelConfig {
                model,
                temperature: None,
                max_tokens: None,
            },
            system_prompt: "You are Night24 Agent Core.".to_string(),
            max_turns: params.limits.max_turns.max(1),
            turn_timeout: Duration::from_millis(params.limits.turn_timeout_ms.max(1)),
            tool_timeout: Duration::from_millis(params.limits.tool_timeout_ms.max(1)),
            total_timeout: Duration::from_millis(params.limits.total_timeout_ms.max(1)),
        };

        let agent = Agent::new(config, provider);
        let now = Utc::now();
        let mut session = Session {
            id: params.session.id.clone(),
            name: params.session.name.clone(),
            session_type: SessionType::User,
            working_dir: params.session.working_dir.clone(),
            conversation: params.session.conversation.clone(),
            created_at: now,
            updated_at: now,
            archived_at: None,
        };
        let user_message = Message {
            id: format!("{}-input", params.run_id),
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: params.input.text.clone(),
            }],
            created_at: Utc::now(),
        };

        agent.run(&mut session, user_message).await
    }

    fn is_initialized(&self) -> bool {
        matches!(self.state, CoreState::Initialized | CoreState::Draining)
    }

    fn success_response(&self, id: JsonRpcId, result: impl serde::Serialize) -> String {
        serialize_response(JsonRpcResponse::success(id, result).unwrap_or_else(|err| {
            JsonRpcResponse::error(
                JsonRpcId::from("rpc-serialization-error"),
                JsonRpcError::internal_error(err.to_string()),
            )
        }))
    }

    fn error_response(&self, id: JsonRpcId, error: JsonRpcError) -> String {
        serialize_response(JsonRpcResponse::error(id, error))
    }
}

fn core_capabilities() -> Vec<Capability> {
    vec![
        Capability::new("core.ping", 1),
        Capability::new("core.shutdown", 1),
        Capability::new("agent.reply", 1),
        Capability::new("agent.tools", 1),
        Capability::new("agent.cancel", 1),
        Capability::new("agent.event", 1),
    ]
}

async fn reply_events(params: ReplyParams, default_provider: String) -> Vec<String> {
    match AgentCore::run_agent(&params, &default_provider).await {
        Ok(messages) => {
            let mut output = Vec::with_capacity(messages.len() + 1);
            let mut seq = 1;
            for message in &messages {
                output.push(agent_event_notification(AgentEvent::new(
                    params.run_id.clone(),
                    seq,
                    AgentEventKind::Message {
                        message: message.clone(),
                    },
                )));
                seq += 1;
            }
            output.push(agent_event_notification(AgentEvent::new(
                params.run_id,
                seq,
                AgentEventKind::Finish {
                    status: FinishStatus::Completed,
                    messages,
                    usage: None,
                },
            )));
            output
        }
        Err(err) => vec![agent_event_notification(AgentEvent::new(
            params.run_id,
            1,
            AgentEventKind::Error {
                code: "internal_error".to_string(),
                message: err.to_string(),
                recoverable: false,
            },
        ))],
    }
}

fn decode_params<T: DeserializeOwned>(
    params: Option<serde_json::Value>,
) -> Result<T, JsonRpcError> {
    let params = params.ok_or_else(|| JsonRpcError::invalid_params("missing params"))?;
    serde_json::from_value(params).map_err(|err| JsonRpcError::invalid_params(err.to_string()))
}

fn decode_optional_params<T>(params: Option<serde_json::Value>) -> Result<T, JsonRpcError>
where
    T: DeserializeOwned + Default,
{
    match params {
        Some(params) => serde_json::from_value(params)
            .map_err(|err| JsonRpcError::invalid_params(err.to_string())),
        None => Ok(T::default()),
    }
}

fn effective_provider(config: &ProviderConfig, default_provider: &str) -> ProviderConfig {
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

fn create_provider(config: &ProviderConfig) -> anyhow::Result<Arc<dyn Provider>> {
    match config.provider.as_str() {
        "echo" => Ok(Arc::new(EchoProvider)),
        "openai" | "openai-responses" => {
            let api_key = resolve_api_key(config, "OPENAI_API_KEY")?;
            let base_url = resolve_base_url(
                config,
                "OPENAI_BASE_URL",
                "https://api.openai.com/v1",
            );
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

fn effective_model(config: &ProviderConfig) -> String {
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

fn agent_event_notification(event: AgentEvent) -> String {
    serialize_notification(
        JsonRpcNotification::new("agent.event", event).unwrap_or_else(|err| JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "agent.event".to_string(),
            params: Some(serde_json::json!({
                "run_id": "run-serialization-error",
                "seq": 1,
                "type": "error",
                "created_at": Utc::now(),
                "payload": {
                    "code": "serialization_error",
                    "message": err.to_string(),
                    "recoverable": false
                }
            })),
        }),
    )
}

fn serialize_response(response: JsonRpcResponse) -> String {
    serde_json::to_string(&response).unwrap_or_else(|err| {
        eprintln!("failed to serialize JSON-RPC response: {err}");
        r#"{"jsonrpc":"2.0","id":"rpc-serialization-error","error":{"code":-32603,"message":"serialization error"}}"#
            .to_string()
    })
}

fn serialize_notification(notification: JsonRpcNotification) -> String {
    serde_json::to_string(&notification).unwrap_or_else(|err| {
        eprintln!("failed to serialize JSON-RPC notification: {err}");
        r#"{"jsonrpc":"2.0","method":"agent.event","params":{"run_id":"run-serialization-error","seq":1,"type":"error","payload":{"code":"serialization_error","message":"serialization error","recoverable":false}}}"#.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn ping_works_before_initialize() {
        let mut core = AgentCore::default();
        let output = core
            .handle_line(
                r#"{"jsonrpc":"2.0","id":"rpc-1","method":"core.ping","params":{"nonce":"abc"}}"#,
            )
            .await;

        assert_eq!(output.len(), 1);
        let value: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
        assert_eq!(value["result"]["nonce"], "abc");
        assert_eq!(value["result"]["status"], "ok");
    }

    #[tokio::test]
    async fn tools_require_initialize() {
        let mut core = AgentCore::default();
        let output = core
            .handle_line(r#"{"jsonrpc":"2.0","id":"rpc-1","method":"agent.tools","params":{}}"#)
            .await;

        let value: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
        assert_eq!(
            value["error"]["code"],
            night24_protocol::CORE_NOT_INITIALIZED
        );
    }

    #[tokio::test]
    async fn initialize_then_tools_returns_builtin_tools() {
        let mut core = initialized_core().await;
        let output = core
            .handle_line(r#"{"jsonrpc":"2.0","id":"rpc-tools","method":"agent.tools","params":{"include_disabled":false}}"#)
            .await;

        let value: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
        let tools = value["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|tool| tool["name"] == "developer__echo"));
    }

    #[tokio::test]
    async fn reply_returns_accepted_message_and_finish() {
        let mut core = initialized_core().await;
        let request = json!({
            "jsonrpc": "2.0",
            "id": "rpc-reply",
            "method": "agent.reply",
            "params": {
                "run_id": "run-1",
                "session": {
                    "id": "session-1",
                    "name": "test",
                    "working_dir": ".",
                    "conversation": []
                },
                "input": { "text": "hello" },
                "provider": { "provider": "echo", "model": "echo-v1" },
                "limits": {
                    "max_turns": 1,
                    "turn_timeout_ms": 10000,
                    "tool_timeout_ms": 10000,
                    "total_timeout_ms": 30000
                },
                "options": {
                    "stream_message_delta": false,
                    "emit_tool_events": true,
                    "permission_mode": "permissive"
                }
            }
        });

        let output = core.handle_line(&request.to_string()).await;

        assert!(output.len() >= 3);
        let accepted: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
        assert_eq!(accepted["result"]["accepted"], true);
        assert_eq!(accepted["result"]["run_id"], "run-1");

        let message: serde_json::Value = serde_json::from_str(&output[1]).unwrap();
        assert_eq!(message["method"], "agent.event");
        assert_eq!(message["params"]["type"], "message");
        assert_eq!(message["params"]["payload"]["message"]["role"], "assistant");

        let finish: serde_json::Value = serde_json::from_str(output.last().unwrap()).unwrap();
        assert_eq!(finish["params"]["type"], "finish");
        assert_eq!(finish["params"]["payload"]["status"], "completed");
    }

    #[test]
    fn stepfun_provider_requires_key_without_falling_back_to_echo() {
        let config = ProviderConfig {
            provider: "stepfun".to_string(),
            model: "step-3.7-flash".to_string(),
            base_url: Some("https://api.stepfun.com/step_plan/v1".to_string()),
            api_key_ref: None,
            api_key: None,
        };

        let error = match create_provider(&config) {
            Ok(_) => panic!("stepfun provider should require an API key"),
            Err(err) => err.to_string(),
        };
        assert!(error.contains("api_key is required for stepfun provider"));
    }

    #[test]
    fn stepfun_provider_uses_inline_request_config() {
        let config = ProviderConfig {
            provider: "stepfun".to_string(),
            model: "step-3.7-flash".to_string(),
            base_url: Some("https://api.stepfun.com/step_plan/v1".to_string()),
            api_key_ref: None,
            api_key: Some("test-key".to_string()),
        };

        let provider = create_provider(&config).unwrap();
        assert_eq!(provider.name(), "openai");
    }

    async fn initialized_core() -> AgentCore {
        let mut core = AgentCore::default();
        let output = core
            .handle_line(
                r#"{"jsonrpc":"2.0","id":"rpc-init","method":"core.initialize","params":{"protocol_version":"2026-07-01","client":{"name":"night24-server","version":"0.1.0"},"capabilities":[]}}"#,
            )
            .await;
        let value: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
        assert_eq!(value["result"]["protocol_version"], "2026-07-01");
        core
    }
}
