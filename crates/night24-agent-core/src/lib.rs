use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use chrono::Utc;
use futures::StreamExt;
use night24_core::model::{ContentBlock, Message, Role};
use night24_core::permission::{PermissionLevel, PermissionManager, ToolCategory};
use night24_core::provider::{
    AnthropicProvider, EchoProvider, ModelConfig, OpenAIProvider, Provider,
};
use night24_core::security::SecurityInspector;
use night24_core::tool_executor::execute_tool;
use night24_core::OllamaProvider;
use night24_protocol::{
    AcceptedResult, AgentEvent, AgentEventKind, AgentToolsParams, AgentToolsResult, CancelParams,
    Capability, EventError, FinishStatus, InitializeParams, InitializeResult, JsonRpcError,
    JsonRpcId, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, PeerInfo, PermissionDecision,
    PermissionResolution, PingParams, PingResult, ProviderConfig, ReplyAccepted, ReplyParams,
    RiskLevel, ShutdownParams, PROTOCOL_VERSION,
};
use serde::de::DeserializeOwned;
use tokio::sync::{mpsc::UnboundedSender, oneshot};
use tokio::time::Instant;

const SERVER_NAME: &str = "night24-agent-core";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoreState {
    Spawned,
    Initialized,
    Draining,
}

#[derive(Clone)]
struct RunHandle {
    cancelled: Arc<AtomicBool>,
}

struct PermissionHandle {
    run_id: String,
    sender: oneshot::Sender<PermissionDecision>,
}

#[derive(Clone)]
struct RunContext {
    run_id: String,
    emit_tool_events: bool,
    cancelled: Arc<AtomicBool>,
    seq: Arc<AtomicU64>,
    output: Option<UnboundedSender<String>>,
    collected: Option<Arc<Mutex<Vec<String>>>>,
    permissions: Arc<Mutex<HashMap<String, PermissionHandle>>>,
}

impl RunContext {
    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }

    fn emit(&self, kind: AgentEventKind) -> String {
        agent_event_notification(AgentEvent::new(self.run_id.clone(), self.next_seq(), kind))
    }

    fn send(&self, kind: AgentEventKind) {
        let message = self.emit(kind);
        if let Some(output) = &self.output {
            let _ = output.send(message);
        } else if let Some(collected) = &self.collected {
            if let Ok(mut collected) = collected.lock() {
                collected.push(message);
            }
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    fn drain_collected(&self) -> Vec<String> {
        self.collected
            .as_ref()
            .and_then(|collected| {
                collected
                    .lock()
                    .ok()
                    .map(|mut values| values.drain(..).collect())
            })
            .unwrap_or_default()
    }
}

pub struct AgentCore {
    state: CoreState,
    exit_after_flush: bool,
    default_provider: String,
    output: Option<UnboundedSender<String>>,
    runs: Arc<Mutex<HashMap<String, RunHandle>>>,
    permissions: Arc<Mutex<HashMap<String, PermissionHandle>>>,
}

impl Default for AgentCore {
    fn default() -> Self {
        Self {
            state: CoreState::Spawned,
            exit_after_flush: false,
            default_provider: "echo".to_string(),
            output: None,
            runs: Arc::new(Mutex::new(HashMap::new())),
            permissions: Arc::new(Mutex::new(HashMap::new())),
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
                if !self.is_initialized() {
                    return vec![self.error_response(id, JsonRpcError::core_not_initialized())];
                }
                self.permission_resolve(id, request.params)
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
        let run_id = params.run_id.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        if let Ok(mut runs) = self.runs.lock() {
            runs.insert(
                run_id.clone(),
                RunHandle {
                    cancelled: cancelled.clone(),
                },
            );
        } else {
            return vec![
                self.error_response(id, JsonRpcError::internal_error("run state lock poisoned"))
            ];
        }

        let accepted = self.success_response(
            id,
            ReplyAccepted {
                accepted: true,
                run_id: run_id.clone(),
            },
        );

        let context = RunContext {
            run_id: run_id.clone(),
            emit_tool_events: params.options.emit_tool_events,
            cancelled,
            seq: Arc::new(AtomicU64::new(1)),
            output: self.output.clone(),
            collected: if self.output.is_none() {
                Some(Arc::new(Mutex::new(Vec::new())))
            } else {
                None
            },
            permissions: self.permissions.clone(),
        };
        let runs = self.runs.clone();
        let permissions = self.permissions.clone();
        let default_provider = self.default_provider.clone();

        if let Some(output) = self.output.clone() {
            tokio::spawn(async move {
                let events = reply_events(params, default_provider, context).await;
                for message in events {
                    let _ = output.send(message);
                }
                cleanup_run(&runs, &permissions, &run_id);
            });
            return vec![accepted];
        }

        let mut output = vec![accepted];
        output.extend(reply_events(params, default_provider, context).await);
        cleanup_run(&runs, &permissions, &run_id);
        output
    }

    fn agent_cancel(&self, id: JsonRpcId, params: Option<serde_json::Value>) -> Vec<String> {
        let params = match decode_params::<CancelParams>(params) {
            Ok(params) => params,
            Err(err) => return vec![self.error_response(id, err)],
        };

        let Some(handle) = self
            .runs
            .lock()
            .ok()
            .and_then(|runs| runs.get(&params.run_id).cloned())
        else {
            return vec![self.error_response(
                id,
                JsonRpcError::new(night24_protocol::RUN_NOT_FOUND, "run not found"),
            )];
        };

        handle.cancelled.store(true, Ordering::SeqCst);
        deny_pending_permissions_for_run(&self.permissions, &params.run_id);

        vec![self.success_response(id, AcceptedResult { accepted: true })]
    }

    fn permission_resolve(&self, id: JsonRpcId, params: Option<serde_json::Value>) -> Vec<String> {
        let params = match decode_params::<PermissionResolution>(params) {
            Ok(params) => params,
            Err(err) => return vec![self.error_response(id, err)],
        };

        let permission = self
            .permissions
            .lock()
            .ok()
            .and_then(|mut permissions| permissions.remove(&params.permission_id));

        let Some(permission) = permission else {
            return vec![self.error_response(
                id,
                JsonRpcError::new(
                    night24_protocol::PERMISSION_REQUEST_NOT_FOUND,
                    "permission request not found",
                ),
            )];
        };

        if permission.run_id != params.run_id {
            return vec![self.error_response(
                id,
                JsonRpcError::new(
                    night24_protocol::PERMISSION_REQUEST_NOT_FOUND,
                    "permission request does not belong to run",
                ),
            )];
        }

        let _ = permission.sender.send(params.decision);
        vec![self.success_response(id, AcceptedResult { accepted: true })]
    }

    async fn run_agent_with_events(
        params: &ReplyParams,
        default_provider: &str,
        context: &RunContext,
    ) -> anyhow::Result<Vec<Message>> {
        let provider_config = effective_provider(&params.provider, default_provider);
        let model = effective_model(&provider_config);
        let provider = create_provider(&provider_config)?;
        let permission_manager =
            permission_manager_for_mode(params.options.permission_mode.as_deref());
        let security = SecurityInspector::new(Arc::new(permission_manager));

        let system = "You are Night24 Agent Core.".to_string();
        let tools = night24_core::tool_executor::builtin_tools();
        let mut messages = params.session.conversation.clone();
        messages.push(Message {
            id: format!("{}-input", params.run_id),
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: params.input.text.clone(),
            }],
            created_at: Utc::now(),
        });

        let mut final_messages = Vec::new();
        let total_deadline = tokio::time::timeout(
            Duration::from_millis(params.limits.total_timeout_ms.max(1)),
            async {
                for _turn in 0..params.limits.max_turns.max(1) {
                    if context.is_cancelled() {
                        anyhow::bail!("cancelled");
                    }

                    let model_config = ModelConfig {
                        model: model.clone(),
                        temperature: None,
                        max_tokens: None,
                    };
                    let turn_result = tokio::time::timeout(
                        Duration::from_millis(params.limits.turn_timeout_ms.max(1)),
                        async {
                            let mut stream = provider
                                .stream(&model_config, &system, &messages, &tools)
                                .await?;
                            let mut message_order = Vec::new();
                            let mut latest_messages: HashMap<String, Message> = HashMap::new();
                            let mut streamed_text: HashMap<String, String> = HashMap::new();
                            let mut has_tool_requests = false;
                            while let Some(result) = stream.next().await {
                                if context.is_cancelled() {
                                    anyhow::bail!("cancelled");
                                }
                                let (message, _usage) = result?;
                                if let Some(message) = message {
                                    has_tool_requests |= message.content.iter().any(|block| {
                                        matches!(block, ContentBlock::ToolRequest { .. })
                                    });
                                    if params.options.stream_message_delta {
                                        let current_text = text_content(&message);
                                        let previous_text =
                                            streamed_text.entry(message.id.clone()).or_default();
                                        if current_text.starts_with(previous_text.as_str())
                                            && current_text.len() > previous_text.len()
                                        {
                                            let delta = current_text[previous_text.len()..].to_string();
                                            context.send(AgentEventKind::MessageDelta {
                                                message_id: message.id.clone(),
                                                delta,
                                            });
                                            *previous_text = current_text;
                                        } else if !current_text.is_empty()
                                            && current_text != *previous_text
                                        {
                                            context.send(AgentEventKind::Message {
                                                message: message.clone(),
                                            });
                                            *previous_text = current_text;
                                        }

                                        if message.content.iter().any(|block| {
                                            matches!(
                                                block,
                                                ContentBlock::ToolRequest { .. }
                                                    | ContentBlock::ToolResponse { .. }
                                            )
                                        }) {
                                            context.send(AgentEventKind::Message {
                                                message: message.clone(),
                                            });
                                        }
                                    } else {
                                        context.send(AgentEventKind::Message {
                                            message: message.clone(),
                                        });
                                    }

                                    if !latest_messages.contains_key(&message.id) {
                                        message_order.push(message.id.clone());
                                    }
                                    latest_messages.insert(message.id.clone(), message);
                                }
                            }

                            let turn_messages = message_order
                                .into_iter()
                                .filter_map(|id| latest_messages.remove(&id))
                                .collect::<Vec<_>>();
                            anyhow::Ok((turn_messages, has_tool_requests))
                        },
                    )
                    .await
                    .map_err(|_| anyhow::anyhow!("agent turn timed out"))??;
                    let (turn_messages, has_tool_requests) = turn_result;

                    if turn_messages.is_empty() {
                        break;
                    }

                    let mut executed_messages = Vec::new();
                    for message in &turn_messages {
                        let mut had_tool_request = false;
                        let mut blocks = Vec::new();
                        for block in &message.content {
                            match block {
                                ContentBlock::ToolRequest {
                                    id,
                                    name,
                                    arguments,
                                } => {
                                    had_tool_request = true;
                                    let started = Instant::now();
                                    let result = execute_tool_with_events(
                                        context,
                                        &security,
                                        id,
                                        name,
                                        arguments,
                                        &params.session.working_dir,
                                        Duration::from_millis(params.limits.tool_timeout_ms.max(1)),
                                    )
                                    .await;
                                    match result {
                                        Ok(content) => {
                                            blocks.push(ContentBlock::ToolResponse {
                                                id: id.clone(),
                                                content,
                                                is_error: false,
                                            });
                                        }
                                        Err(err) => {
                                            let content = format!("error: {err}");
                                            if context.emit_tool_events {
                                                context.send(AgentEventKind::ToolFailed {
                                                    tool_call_id: id.clone(),
                                                    tool_name: name.clone(),
                                                    duration_ms: started.elapsed().as_millis()
                                                        as u64,
                                                    error: EventError {
                                                        code: if context.is_cancelled() {
                                                            "cancelled".to_string()
                                                        } else {
                                                            "tool_execution_failed".to_string()
                                                        },
                                                        message: err.to_string(),
                                                    },
                                                });
                                            }
                                            blocks.push(ContentBlock::ToolResponse {
                                                id: id.clone(),
                                                content,
                                                is_error: true,
                                            });
                                        }
                                    }
                                }
                                other => blocks.push(other.clone()),
                            }
                        }
                        if had_tool_request && !blocks.is_empty() {
                            executed_messages.push(Message {
                                id: message.id.clone(),
                                role: message.role,
                                content: blocks,
                                created_at: message.created_at,
                            });
                        }
                    }

                    messages.extend(turn_messages.clone());
                    messages.extend(executed_messages.clone());
                    final_messages.extend(turn_messages);
                    final_messages.extend(executed_messages);

                    if !has_tool_requests {
                        break;
                    }
                }
                anyhow::Ok(())
            },
        )
        .await;

        match total_deadline {
            Ok(Ok(())) => Ok(final_messages),
            Ok(Err(err)) => Err(err),
            Err(_) => anyhow::bail!("agent run timed out"),
        }
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
        Capability::new("permission.resolve", 1),
    ]
}

async fn reply_events(
    params: ReplyParams,
    default_provider: String,
    context: RunContext,
) -> Vec<String> {
    match AgentCore::run_agent_with_events(&params, &default_provider, &context).await {
        Ok(messages) => {
            let mut output = context.drain_collected();
            output.push(agent_event_notification(AgentEvent::new(
                params.run_id.clone(),
                context.next_seq(),
                AgentEventKind::Finish {
                    status: if context.is_cancelled() {
                        FinishStatus::Cancelled
                    } else {
                        FinishStatus::Completed
                    },
                    messages,
                    usage: None,
                },
            )));
            output
        }
        Err(err) => {
            let message = err.to_string();
            let mut output = context.drain_collected();
            if context.is_cancelled() || message.contains("cancelled") {
                output.push(agent_event_notification(AgentEvent::new(
                    params.run_id,
                    context.next_seq(),
                    AgentEventKind::Finish {
                        status: FinishStatus::Cancelled,
                        messages: Vec::new(),
                        usage: None,
                    },
                )));
                output
            } else {
                output.push(agent_event_notification(AgentEvent::new(
                    params.run_id,
                    context.next_seq(),
                    AgentEventKind::Error {
                        code: "internal_error".to_string(),
                        message,
                        recoverable: false,
                    },
                )));
                output
            }
        }
    }
}

async fn execute_tool_with_events(
    context: &RunContext,
    security: &SecurityInspector,
    tool_call_id: &str,
    tool_name: &str,
    arguments: &serde_json::Value,
    working_dir: &std::path::Path,
    tool_timeout: Duration,
) -> anyhow::Result<String> {
    if context.is_cancelled() {
        anyhow::bail!("cancelled");
    }

    match security.require_permission(tool_name).await {
        PermissionLevel::Deny => anyhow::bail!("permission denied for {tool_name}"),
        PermissionLevel::Confirm => {
            let permission_id = format!("perm-{}", uuid::Uuid::new_v4());
            let (tx, rx) = oneshot::channel();
            context
                .permissions
                .lock()
                .map_err(|_| anyhow::anyhow!("permission state lock poisoned"))?
                .insert(
                    permission_id.clone(),
                    PermissionHandle {
                        run_id: context.run_id.clone(),
                        sender: tx,
                    },
                );

            context.send(AgentEventKind::PermissionRequired {
                permission_id,
                tool_call_id: tool_call_id.to_string(),
                tool_name: tool_name.to_string(),
                risk: risk_for_tool(tool_name),
                summary: summarize_tool_call(tool_name, arguments),
                arguments: arguments.clone(),
                timeout_ms: 300_000,
            });

            let decision = tokio::time::timeout(Duration::from_secs(300), rx)
                .await
                .map_err(|_| anyhow::anyhow!("permission request timed out"))?
                .map_err(|_| anyhow::anyhow!("permission request closed"))?;

            if matches!(decision, PermissionDecision::Deny) {
                anyhow::bail!("permission denied for {tool_name}");
            }
        }
        PermissionLevel::Allow => {}
    }

    if context.is_cancelled() {
        anyhow::bail!("cancelled");
    }

    if context.emit_tool_events {
        context.send(AgentEventKind::ToolStarted {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            summary: summarize_tool_call(tool_name, arguments),
            arguments: arguments.clone(),
        });
    }

    let started = Instant::now();
    let result = tokio::time::timeout(
        tool_timeout,
        execute_tool(tool_name, arguments, working_dir, security),
    )
    .await
    .map_err(|_| anyhow::anyhow!("tool timed out after {:?}", tool_timeout))??;

    if context.is_cancelled() {
        anyhow::bail!("cancelled");
    }

    if context.emit_tool_events {
        context.send(AgentEventKind::ToolFinished {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            duration_ms: started.elapsed().as_millis() as u64,
            summary: format!("{} completed", tool_name),
            result_preview: preview(&result),
            is_error: false,
        });
    }

    Ok(result)
}

fn permission_manager_for_mode(mode: Option<&str>) -> PermissionManager {
    match mode
        .unwrap_or("strict")
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .as_str()
    {
        "allow_all" | "full_access" => PermissionManager::new(PermissionLevel::Allow),
        "deny_all" => PermissionManager::new(PermissionLevel::Deny),
        "permissive" => PermissionManager::permissive_local(),
        _ => PermissionManager::default(),
    }
}

fn text_content(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } | ContentBlock::Thinking { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn risk_for_tool(tool_name: &str) -> RiskLevel {
    match ToolCategory::from_tool_name(tool_name) {
        ToolCategory::Shell | ToolCategory::Write => RiskLevel::High,
        ToolCategory::Network => RiskLevel::Medium,
        ToolCategory::Read | ToolCategory::Other => RiskLevel::Low,
    }
}

fn summarize_tool_call(tool_name: &str, arguments: &serde_json::Value) -> String {
    match tool_name {
        "developer__shell" => arguments
            .get("command")
            .and_then(|value| value.as_str())
            .map(|value| format!("Run shell command: {}", value))
            .unwrap_or_else(|| "Run shell command".to_string()),
        "developer__write_file" => arguments
            .get("path")
            .and_then(|value| value.as_str())
            .map(|value| format!("Write file: {}", value))
            .unwrap_or_else(|| "Write file".to_string()),
        "developer__read_file" => arguments
            .get("path")
            .and_then(|value| value.as_str())
            .map(|value| format!("Read file: {}", value))
            .unwrap_or_else(|| "Read file".to_string()),
        _ => format!("Call {}", tool_name),
    }
}

fn preview(text: &str) -> String {
    const MAX_PREVIEW: usize = 500;
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= MAX_PREVIEW {
        compact
    } else {
        compact.chars().take(MAX_PREVIEW).collect::<String>() + "..."
    }
}

fn cleanup_run(
    runs: &Arc<Mutex<HashMap<String, RunHandle>>>,
    permissions: &Arc<Mutex<HashMap<String, PermissionHandle>>>,
    run_id: &str,
) {
    if let Ok(mut runs) = runs.lock() {
        runs.remove(run_id);
    }
    deny_pending_permissions_for_run(permissions, run_id);
}

fn deny_pending_permissions_for_run(
    permissions: &Arc<Mutex<HashMap<String, PermissionHandle>>>,
    run_id: &str,
) {
    let pending = permissions
        .lock()
        .ok()
        .map(|mut permissions| {
            let ids = permissions
                .iter()
                .filter_map(|(id, handle)| {
                    if handle.run_id == run_id {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| permissions.remove(&id))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for permission in pending {
        let _ = permission.sender.send(PermissionDecision::Deny);
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

    #[tokio::test]
    async fn strict_tool_call_waits_for_permission_and_continues_after_approve() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut core = AgentCore::with_output(tx);
        initialize_core(&mut core).await;

        let request = json!({
            "jsonrpc": "2.0",
            "id": "rpc-reply",
            "method": "agent.reply",
            "params": {
                "run_id": "run-permission",
                "session": {
                    "id": "session-1",
                    "name": "test",
                    "working_dir": ".",
                    "conversation": []
                },
                "input": { "text": "tool:datetime:" },
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
                    "permission_mode": "strict"
                }
            }
        });

        let accepted = core.handle_line(&request.to_string()).await;
        let accepted: serde_json::Value = serde_json::from_str(&accepted[0]).unwrap();
        assert_eq!(accepted["result"]["accepted"], true);

        let permission = next_event_of_type(&mut rx, "permission_required").await;
        let permission_id = permission["params"]["payload"]["permission_id"]
            .as_str()
            .unwrap()
            .to_string();

        let resolve = json!({
            "jsonrpc": "2.0",
            "id": "rpc-permission",
            "method": "permission.resolve",
            "params": {
                "run_id": "run-permission",
                "permission_id": permission_id,
                "decision": "approve"
            }
        });
        let resolved = core.handle_line(&resolve.to_string()).await;
        let resolved: serde_json::Value = serde_json::from_str(&resolved[0]).unwrap();
        assert_eq!(resolved["result"]["accepted"], true);

        let tool_started = next_event_of_type(&mut rx, "tool_started").await;
        assert_eq!(
            tool_started["params"]["payload"]["tool_name"],
            "developer__datetime"
        );
        let finish = next_event_of_type(&mut rx, "finish").await;
        assert_eq!(finish["params"]["payload"]["status"], "completed");
    }

    #[tokio::test]
    async fn cancel_unblocks_pending_permission_and_finishes_cancelled() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut core = AgentCore::with_output(tx);
        initialize_core(&mut core).await;

        let request = json!({
            "jsonrpc": "2.0",
            "id": "rpc-reply",
            "method": "agent.reply",
            "params": {
                "run_id": "run-cancel",
                "session": {
                    "id": "session-1",
                    "name": "test",
                    "working_dir": ".",
                    "conversation": []
                },
                "input": { "text": "tool:datetime:" },
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
                    "permission_mode": "strict"
                }
            }
        });

        let accepted = core.handle_line(&request.to_string()).await;
        let accepted: serde_json::Value = serde_json::from_str(&accepted[0]).unwrap();
        assert_eq!(accepted["result"]["accepted"], true);
        let _permission = next_event_of_type(&mut rx, "permission_required").await;

        let cancel = json!({
            "jsonrpc": "2.0",
            "id": "rpc-cancel",
            "method": "agent.cancel",
            "params": {
                "run_id": "run-cancel",
                "reason": "test"
            }
        });
        let cancelled = core.handle_line(&cancel.to_string()).await;
        let cancelled: serde_json::Value = serde_json::from_str(&cancelled[0]).unwrap();
        assert_eq!(cancelled["result"]["accepted"], true);

        let finish = next_event_of_type(&mut rx, "finish").await;
        assert_eq!(finish["params"]["payload"]["status"], "cancelled");
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
        initialize_core(&mut core).await;
        core
    }

    async fn initialize_core(core: &mut AgentCore) {
        let output = core
            .handle_line(
                r#"{"jsonrpc":"2.0","id":"rpc-init","method":"core.initialize","params":{"protocol_version":"2026-07-01","client":{"name":"night24-server","version":"0.1.0"},"capabilities":[]}}"#,
            )
            .await;
        let value: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
        assert_eq!(value["result"]["protocol_version"], "2026-07-01");
    }

    async fn next_event_of_type(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
        event_type: &str,
    ) -> serde_json::Value {
        for _ in 0..20 {
            let raw = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .expect("timed out waiting for agent event")
                .expect("agent event channel closed");
            let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
            if value["params"]["type"] == event_type {
                return value;
            }
        }
        panic!("event type {event_type} was not emitted");
    }
}
