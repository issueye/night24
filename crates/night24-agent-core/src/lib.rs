use std::collections::HashMap;
mod hooks;
mod providers;
mod rpc;
mod skills;
mod subagents;
mod task_list;
mod tools;
mod types;

use hooks::{HookContext, HookEvent, HookRunner};
use providers::{create_provider, effective_model, effective_provider};
use rpc::{
    agent_event_notification, core_capabilities, decode_optional_params, decode_params,
    serialize_response,
};
use skills::SkillRegistry;
use task_list::{is_task_list_tool, summarize_task_list_tool, TaskListState};
#[cfg(test)]
use tools::arguments_with_network_proxy;
use tools::{
    cleanup_run, deny_pending_permissions_for_run, ensure_tool_permission,
    execute_tool_with_events, permission_manager_for_mode, text_content, ToolLifecycle,
};
use types::{CoreState, PermissionHandle, RunContext, RunHandle};

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use chrono::Utc;
use futures::StreamExt;
use night24_core::model::{ContentBlock, Message, Role, Tool};
use night24_core::provider::{clamp_request_retries, ModelConfig, Provider};
use night24_core::security::SecurityInspector;
#[cfg(test)]
use night24_protocol::ProviderConfig;
use night24_protocol::{
    AcceptedResult, AgentEvent, AgentEventKind, AgentToolsParams, AgentToolsResult, CancelParams,
    EventError, FinishStatus, InitializeParams, InitializeResult, JsonRpcError, JsonRpcId,
    JsonRpcRequest, JsonRpcResponse, OutputStream, PeerInfo, PermissionResolution, PingParams,
    PingResult, ReplyAccepted, ReplyParams, ShutdownParams, SkillLoadParams, SkillLoadResult,
    SkillRegistryParams, SkillRegistryResult, SubAgentPoolParams, SubAgentPoolResult,
    PROTOCOL_VERSION,
};
use subagents::{SubAgentMessageDirection, SubAgentMode, SubAgentPool};
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::Instant;

const SERVER_NAME: &str = "night24-agent-core";
const PROVIDER_TURN_RETRY_SOURCE: &str = "provider_retry";

pub struct AgentCore {
    state: CoreState,
    exit_after_flush: bool,
    default_provider: String,
    output: Option<UnboundedSender<String>>,
    runs: Arc<Mutex<HashMap<String, RunHandle>>>,
    permissions: Arc<Mutex<HashMap<String, PermissionHandle>>>,
    subagents: Arc<SubAgentPool>,
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
            subagents: Arc::new(SubAgentPool::default()),
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
            "agent.subagents" => {
                if !self.is_initialized() {
                    return vec![self.error_response(id, JsonRpcError::core_not_initialized())];
                }
                self.agent_subagents(id, request.params)
            }
            "agent.skills" => {
                if !self.is_initialized() {
                    return vec![self.error_response(id, JsonRpcError::core_not_initialized())];
                }
                self.agent_skills(id, request.params)
            }
            "agent.skill.load" => {
                if !self.is_initialized() {
                    return vec![self.error_response(id, JsonRpcError::core_not_initialized())];
                }
                self.agent_skill_load(id, request.params)
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

        let skills = Arc::new(SkillRegistry::load(&params.session.working_dir));

        let context = RunContext {
            run_id: run_id.clone(),
            agent_id: None,
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
            hooks: Arc::new(HookRunner::from_environment(&params.session.working_dir)),
            subagents: self.subagents.clone(),
            skills,
            task_list: Arc::new(Mutex::new(TaskListState::default())),
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

    fn agent_subagents(&self, id: JsonRpcId, params: Option<serde_json::Value>) -> Vec<String> {
        let params = match decode_optional_params::<SubAgentPoolParams>(params) {
            Ok(params) => params,
            Err(err) => return vec![self.error_response(id, err)],
        };

        let pool = match self.subagents.snapshot(
            params.subagent_id.as_deref(),
            params.parent_session_id.as_deref(),
            params.include_messages,
            params.include_result,
        ) {
            Ok(pool) => pool,
            Err(err) => {
                return vec![self.error_response(id, JsonRpcError::invalid_params(err.to_string()))]
            }
        };

        vec![self.success_response(id, SubAgentPoolResult { pool })]
    }

    fn agent_skills(&self, id: JsonRpcId, params: Option<serde_json::Value>) -> Vec<String> {
        let params = match decode_optional_params::<SkillRegistryParams>(params) {
            Ok(params) => params,
            Err(err) => return vec![self.error_response(id, err)],
        };
        let working_dir = params
            .working_dir
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));
        let registry = SkillRegistry::load(&working_dir);
        let registry = match serde_json::to_value(registry) {
            Ok(registry) => registry,
            Err(err) => {
                return vec![self.error_response(id, JsonRpcError::internal_error(err.to_string()))]
            }
        };
        vec![self.success_response(id, SkillRegistryResult { registry })]
    }

    fn agent_skill_load(&self, id: JsonRpcId, params: Option<serde_json::Value>) -> Vec<String> {
        let params = match decode_params::<SkillLoadParams>(params) {
            Ok(params) => params,
            Err(err) => return vec![self.error_response(id, err)],
        };
        let working_dir = params
            .working_dir
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));
        let registry = SkillRegistry::load(&working_dir);
        let skill = match registry.load_skill(&params.name, params.file.as_deref()) {
            Ok(skill) => skill,
            Err(err) => {
                return vec![self.error_response(id, JsonRpcError::invalid_params(err.to_string()))]
            }
        };
        let skill = match serde_json::to_value(skill) {
            Ok(skill) => skill,
            Err(err) => {
                return vec![self.error_response(id, JsonRpcError::internal_error(err.to_string()))]
            }
        };
        vec![self.success_response(id, SkillLoadResult { skill })]
    }

    fn permission_resolve(&self, id: JsonRpcId, params: Option<serde_json::Value>) -> Vec<String> {
        let params = match decode_params::<PermissionResolution>(params) {
            Ok(params) => params,
            Err(err) => return vec![self.error_response(id, err)],
        };

        let permission =
            match take_permission_for_run(&self.permissions, &params.permission_id, &params.run_id)
            {
                Ok(permission) => permission,
                Err(err) => return vec![self.error_response(id, err)],
            };

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

        let system = build_system_prompt(context, &params.input.text);
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
                let mut awaiting_tool_followup = false;
                for turn in 0..params.limits.max_turns.max(1) {
                    if context.is_cancelled() {
                        anyhow::bail!("cancelled");
                    }

                    let model_config = ModelConfig {
                        model: model.clone(),
                        temperature: None,
                        max_tokens: None,
                        request_retries: clamp_request_retries(params.options.request_retries),
                    };
                    let turn_result = run_provider_turn_with_retries(
                        context,
                        &params.run_id,
                        &params.session.working_dir,
                        provider.as_ref(),
                        &model_config,
                        &system,
                        &messages,
                        &tools,
                        params.options.stream_message_delta,
                        params.limits.turn_timeout_ms,
                    )
                    .await?;
                    let ProviderTurn {
                        messages: turn_messages,
                        has_tool_requests,
                    } = turn_result;

                    if turn_messages.is_empty() {
                        if awaiting_tool_followup {
                            anyhow::bail!(
                                "模型在工具结果返回后没有继续生成回复。可能是模型服务未接受工具结果，或返回了空响应。"
                            );
                        }
                        break;
                    }

                    let executed_messages = Self::execute_turn_tools(
                        params,
                        default_provider,
                        context,
                        &security,
                        &turn_messages,
                    )
                    .await;
                    let should_continue_tasks = !has_tool_requests
                        && (should_continue_task_workflow(&turn_messages)
                            || context_has_open_task_list(context));

                    messages.extend(turn_messages.clone());
                    messages.extend(executed_messages.clone());
                    final_messages.extend(turn_messages);
                    final_messages.extend(executed_messages);

                    if !has_tool_requests {
                        if should_continue_tasks {
                            messages.push(task_workflow_continue_message(&params.run_id, turn));
                            awaiting_tool_followup = false;
                            continue;
                        }
                        break;
                    }
                    awaiting_tool_followup = true;
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

    async fn execute_turn_tools(
        params: &ReplyParams,
        default_provider: &str,
        context: &RunContext,
        security: &SecurityInspector,
        turn_messages: &[Message],
    ) -> Vec<Message> {
        let mut executed_messages = Vec::new();
        for message in turn_messages {
            let mut blocks = Vec::new();
            for block in &message.content {
                if let ContentBlock::ToolRequest {
                    id,
                    name,
                    arguments,
                } = block
                {
                    blocks.push(
                        Self::execute_tool_request_response(
                            params,
                            default_provider,
                            context,
                            security,
                            id,
                            name,
                            arguments,
                        )
                        .await,
                    );
                }
            }

            if !blocks.is_empty() {
                executed_messages.push(Message {
                    id: format!("{}-tool-response-{}", message.id, executed_messages.len()),
                    role: Role::Tool,
                    content: blocks,
                    created_at: Utc::now(),
                });
            }
        }

        executed_messages
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_tool_request_response(
        params: &ReplyParams,
        default_provider: &str,
        context: &RunContext,
        security: &SecurityInspector,
        tool_call_id: &str,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> ContentBlock {
        if let Some(message) = recoverable_tool_argument_message(tool_name, arguments) {
            return ContentBlock::ToolResponse {
                id: tool_call_id.to_string(),
                content: message,
                is_error: false,
            };
        }

        let started = Instant::now();
        let result = if is_subagent_tool(tool_name) {
            Self::execute_subagent_tool_with_events(
                params,
                default_provider,
                context,
                security,
                tool_call_id,
                tool_name,
                arguments,
            )
            .await
        } else if is_skill_tool(tool_name) {
            Self::execute_skill_tool_with_events(
                params,
                context,
                security,
                tool_call_id,
                tool_name,
                arguments,
            )
            .await
        } else if is_task_list_tool(tool_name) {
            Self::execute_task_list_tool_with_events(
                params,
                context,
                tool_call_id,
                tool_name,
                arguments,
            )
            .await
        } else {
            execute_tool_with_events(
                context,
                security,
                tool_call_id,
                tool_name,
                arguments,
                &params.session.working_dir,
                Duration::from_millis(params.limits.tool_timeout_ms.max(1)),
                params.options.network_proxy.as_deref(),
            )
            .await
        };

        match result {
            Ok(content) => ContentBlock::ToolResponse {
                id: tool_call_id.to_string(),
                content,
                is_error: false,
            },
            Err(err) => {
                let content = format!("error: {err}");
                if context.emit_tool_events {
                    context.send(AgentEventKind::ToolFailed {
                        tool_call_id: tool_call_id.to_string(),
                        tool_name: tool_name.to_string(),
                        duration_ms: started.elapsed().as_millis() as u64,
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
                ContentBlock::ToolResponse {
                    id: tool_call_id.to_string(),
                    content,
                    is_error: true,
                }
            }
        }
    }

    async fn execute_subagent_tool_with_events(
        params: &ReplyParams,
        default_provider: &str,
        context: &RunContext,
        security: &SecurityInspector,
        tool_call_id: &str,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> anyhow::Result<String> {
        let summary = summarize_subagent_tool(tool_name, arguments);
        ensure_tool_permission(
            context,
            security,
            &params.session.working_dir,
            tool_call_id,
            tool_name,
            arguments,
            &summary,
        )
        .await?;

        let lifecycle = ToolLifecycle::new(
            context,
            &params.session.working_dir,
            tool_call_id,
            tool_name,
            arguments,
        );
        lifecycle.before(&summary).await;

        let started = Instant::now();
        let result =
            execute_subagent_tool(params, default_provider, context, tool_name, arguments).await;
        let duration_ms = started.elapsed().as_millis() as u64;

        match result {
            Ok(output) => {
                let result_preview = preview_text(&output);
                lifecycle
                    .success(
                        duration_ms,
                        format!("{tool_name} completed"),
                        result_preview,
                    )
                    .await;
                Ok(output)
            }
            Err(err) => {
                let error = err.to_string();
                lifecycle
                    .failure(duration_ms, "subagent_tool_failed", &error)
                    .await;
                Err(err)
            }
        }
    }

    async fn execute_skill_tool_with_events(
        params: &ReplyParams,
        context: &RunContext,
        security: &SecurityInspector,
        tool_call_id: &str,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> anyhow::Result<String> {
        let summary = summarize_skill_tool(arguments);
        ensure_tool_permission(
            context,
            security,
            &params.session.working_dir,
            tool_call_id,
            tool_name,
            arguments,
            &summary,
        )
        .await?;

        let lifecycle = ToolLifecycle::new(
            context,
            &params.session.working_dir,
            tool_call_id,
            tool_name,
            arguments,
        );
        lifecycle.before(&summary).await;

        let started = Instant::now();
        let result = execute_skill_tool(context, arguments).await;
        let duration_ms = started.elapsed().as_millis() as u64;

        match result {
            Ok(output) => {
                let result_preview = preview_text(&output);
                lifecycle
                    .success(
                        duration_ms,
                        "developer__skill_load completed".to_string(),
                        result_preview,
                    )
                    .await;
                Ok(output)
            }
            Err(err) => {
                let error = err.to_string();
                lifecycle
                    .failure(duration_ms, "skill_tool_failed", &error)
                    .await;
                Err(err)
            }
        }
    }

    async fn execute_task_list_tool_with_events(
        params: &ReplyParams,
        context: &RunContext,
        tool_call_id: &str,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> anyhow::Result<String> {
        let summary = summarize_task_list_tool(tool_name, arguments);
        let lifecycle = ToolLifecycle::new(
            context,
            &params.session.working_dir,
            tool_call_id,
            tool_name,
            arguments,
        );
        lifecycle.before(&summary).await;

        let started = Instant::now();
        let result = {
            let mut task_list = context
                .task_list
                .lock()
                .map_err(|_| anyhow::anyhow!("task list state lock poisoned"))?;
            task_list.execute(tool_name, arguments)
        };
        let duration_ms = started.elapsed().as_millis() as u64;
        let markdown = match result {
            Ok(markdown) => markdown,
            Err(err) => {
                let error = err.to_string();
                lifecycle
                    .failure(duration_ms, "task_list_tool_failed", &error)
                    .await;
                return Err(err);
            }
        };

        context.send(AgentEventKind::Message {
            message: Message {
                id: format!("{}-{tool_call_id}-task-list", context.run_id),
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: markdown.clone(),
                }],
                created_at: Utc::now(),
            },
        });

        lifecycle
            .success(
                duration_ms,
                format!("{tool_name} completed"),
                preview_text(&markdown),
            )
            .await;
        Ok(markdown)
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

struct ProviderTurn {
    messages: Vec<Message>,
    has_tool_requests: bool,
}

#[allow(clippy::too_many_arguments)]
async fn run_provider_turn_with_retries(
    context: &RunContext,
    run_id: &str,
    working_dir: &std::path::Path,
    provider: &dyn Provider,
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    stream_message_delta: bool,
    turn_timeout_ms: u64,
) -> anyhow::Result<ProviderTurn> {
    let max_retries = model_config.request_retries;
    let mut retries_done = 0;

    loop {
        let result = tokio::time::timeout(
            Duration::from_millis(turn_timeout_ms.max(1)),
            run_provider_turn(
                context,
                run_id,
                working_dir,
                provider,
                model_config,
                system,
                messages,
                tools,
                stream_message_delta,
            ),
        )
        .await
        .map_err(|_| anyhow::anyhow!("agent turn timed out"))
        .and_then(|result| result);

        match result {
            Ok(turn) => return Ok(turn),
            Err(err) => {
                if context.is_cancelled() {
                    return Err(err);
                }
                if retries_done >= max_retries || !is_retryable_provider_turn_error(&err) {
                    return Err(err);
                }
                retries_done += 1;
                context.send(AgentEventKind::RunOutput {
                    source: PROVIDER_TURN_RETRY_SOURCE.to_string(),
                    stream: OutputStream::Stderr,
                    text: format!(
                        "模型请求失败，正在重试 {}/{}：{}",
                        retries_done, max_retries, err
                    ),
                });
                sleep_before_provider_turn_retry(retries_done).await;
            }
        }
    }
}

fn is_retryable_provider_turn_error(err: &anyhow::Error) -> bool {
    let message = err.to_string().to_ascii_lowercase();
    if message.contains("cancelled")
        || message.contains("401")
        || message.contains("403")
        || message.contains("forbidden")
        || message.contains("unauthorized")
        || message.contains("insufficient balance")
        || message.contains("invalid api key")
        || message.contains("requires a base url that supports post /responses")
        || message.contains("switch the provider")
    {
        return false;
    }

    message.contains("network error")
        || message.contains("timed out")
        || message.contains("timeout")
        || message.contains("connection")
        || message.contains("upstream request failed")
        || message.contains("too many requests")
        || message.contains("429")
        || message.contains("500")
        || message.contains("502")
        || message.contains("503")
        || message.contains("504")
        || message.contains("bad gateway")
        || message.contains("service unavailable")
}

async fn sleep_before_provider_turn_retry(retry_number: u8) {
    let delay_ms = match retry_number {
        0 | 1 => 300,
        2 => 700,
        3 => 1_500,
        4 => 3_000,
        _ => 5_000,
    };
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
}

#[allow(clippy::too_many_arguments)]
async fn run_provider_turn(
    context: &RunContext,
    run_id: &str,
    working_dir: &std::path::Path,
    provider: &dyn Provider,
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    stream_message_delta: bool,
) -> anyhow::Result<ProviderTurn> {
    context
        .run_hooks(HookContext {
            event: HookEvent::BeforeProviderRequest,
            run_id,
            working_dir,
            provider: Some(provider.name()),
            model: Some(&model_config.model),
            message_count: Some(messages.len()),
            tool_count: Some(tools.len()),
            tool_call_id: None,
            tool_name: None,
            summary: None,
            arguments: None,
            result_preview: None,
            error: None,
            duration_ms: None,
            finish_status: None,
        })
        .await;

    let mut stream = provider
        .stream(model_config, system, messages, tools)
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
            has_tool_requests |= message
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolRequest { .. }));
            emit_provider_message(context, &mut streamed_text, &message, stream_message_delta);

            if !latest_messages.contains_key(&message.id) {
                message_order.push(message.id.clone());
            }
            latest_messages.insert(message.id.clone(), message);
        }
    }

    Ok(ProviderTurn {
        messages: message_order
            .into_iter()
            .filter_map(|id| latest_messages.remove(&id))
            .collect(),
        has_tool_requests,
    })
}

fn take_permission_for_run(
    permissions: &Arc<Mutex<HashMap<String, PermissionHandle>>>,
    permission_id: &str,
    run_id: &str,
) -> Result<PermissionHandle, JsonRpcError> {
    let mut permissions = permissions
        .lock()
        .map_err(|_| JsonRpcError::internal_error("permission state lock poisoned"))?;
    let Some(permission) = permissions.get(permission_id) else {
        return Err(JsonRpcError::new(
            night24_protocol::PERMISSION_REQUEST_NOT_FOUND,
            "permission request not found",
        ));
    };
    if permission.run_id != run_id {
        return Err(JsonRpcError::new(
            night24_protocol::PERMISSION_REQUEST_NOT_FOUND,
            "permission request does not belong to run",
        ));
    }
    permissions
        .remove(permission_id)
        .ok_or_else(|| JsonRpcError::internal_error("permission request disappeared"))
}

fn emit_provider_message(
    context: &RunContext,
    streamed_text: &mut HashMap<String, String>,
    message: &Message,
    stream_message_delta: bool,
) {
    if !stream_message_delta {
        context.send(AgentEventKind::Message {
            message: message.clone(),
        });
        return;
    }

    let current_text = text_content(message);
    let previous_text = streamed_text.entry(message.id.clone()).or_default();
    if current_text.starts_with(previous_text.as_str()) && current_text.len() > previous_text.len()
    {
        let delta = current_text[previous_text.len()..].to_string();
        context.send(AgentEventKind::MessageDelta {
            message_id: message.id.clone(),
            delta,
        });
        *previous_text = current_text;
    } else if !current_text.is_empty() && current_text != *previous_text {
        context.send(AgentEventKind::Message {
            message: message.clone(),
        });
        *previous_text = current_text;
    }

    if message.content.iter().any(|block| {
        matches!(
            block,
            ContentBlock::ToolRequest { .. } | ContentBlock::ToolResponse { .. }
        )
    }) {
        context.send(AgentEventKind::Message {
            message: message.clone(),
        });
    }
}

fn is_subagent_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "developer__subagent_spawn"
            | "developer__subagent_status"
            | "developer__subagent_message"
            | "developer__subagent_wait"
            | "developer__subagent_cancel"
    )
}

fn is_skill_tool(tool_name: &str) -> bool {
    tool_name == "developer__skill_load"
}

fn recoverable_tool_argument_message(
    tool_name: &str,
    arguments: &serde_json::Value,
) -> Option<String> {
    let requirements: &[(&str, &[&str], &str)] = match tool_name {
        "developer__shell" => &[("command", &["command"], "the shell command to run")],
        "developer__read_file" => &[(
            "path",
            &["path", "file_path", "filepath"],
            "the relative file path to read",
        )],
        "developer__write_file" => &[
            (
                "path",
                &["path", "file_path", "filepath", "target_path"],
                "the relative target file path",
            ),
            (
                "content",
                &["content"],
                "the complete file content to write",
            ),
        ],
        "developer__file_search" => &[("query", &["query"], "the text pattern to search for")],
        "developer__http_request" | "developer__network_request" => {
            &[("url", &["url"], "the HTTP or HTTPS URL")]
        }
        "developer__web_search" | "developer__network_search" => {
            &[("query", &["query"], "the web search query")]
        }
        "developer__web_scraper" => &[("url", &["url"], "the page URL to scrape")],
        "developer__calculator" => &[("expression", &["expression"], "the math expression")],
        "developer__jq" => &[
            ("data", &["data"], "the JSON input data"),
            ("query", &["query"], "the jq-like query"),
        ],
        "developer__code_interpreter" => &[("code", &["code"], "the code snippet to execute")],
        "developer__database_query" => &[("query", &["query"], "the read-only SQL query")],
        _ => return None,
    };

    let missing = requirements
        .iter()
        .filter(|(_, keys, _)| first_string_arg(arguments, keys).is_none())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return None;
    }

    let missing_names = missing
        .iter()
        .map(|(name, _, _)| *name)
        .collect::<Vec<_>>()
        .join(", ");
    let required_detail = requirements
        .iter()
        .map(|(name, _, description)| format!("{name}: {description}"))
        .collect::<Vec<_>>()
        .join("; ");

    Some(format!(
        "工具参数不完整，未执行 {tool_name}。缺少字段: {missing_names}。请不要再次用空参数调用该工具。请先从当前上下文重新构造所需参数；如果上下文不足，先调用读取/搜索类工具获取内容，然后再用完整参数重试。Required arguments: {required_detail}."
    ))
}

fn first_string_arg<'a>(arguments: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        arguments
            .get(*key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

const TASK_WORKFLOW_PROMPT: &str = r#"For complex tasks, use the Night24 task workflow:
- First decompose the request before taking implementation action.
- Create the task list by calling `developer__task_list_create`; do not write the initial task list manually.
- Keep the task list current with `developer__task_list_update` after each meaningful step.
- Work step by step according to that task list.
- When the task is finished, call `developer__task_list_finish` with the final completion report.
- In the completion report, summarize completed work, verification performed, and any remaining risks or follow-up notes.
- Do not stop after only creating the task list; continue executing the first unfinished task in the same run.
- For trivial one-step questions or direct answers, do not add this workflow unless it improves clarity."#;

fn should_continue_task_workflow(messages: &[Message]) -> bool {
    messages.iter().rev().any(message_has_open_task_list)
}

fn context_has_open_task_list(context: &RunContext) -> bool {
    context
        .task_list
        .lock()
        .map(|task_list| task_list.has_open_tasks())
        .unwrap_or(false)
}

fn message_has_open_task_list(message: &Message) -> bool {
    if message.role != Role::Assistant {
        return false;
    }
    let text = text_content(message);
    task_workflow_state(&text).is_some_and(|state| state.has_open_tasks && !state.has_report)
}

#[derive(Debug, Clone, Copy, Default)]
struct TaskWorkflowState {
    has_open_tasks: bool,
    has_report: bool,
}

fn task_workflow_state(text: &str) -> Option<TaskWorkflowState> {
    let mut in_task_list = false;
    let mut saw_task_list = false;
    let mut state = TaskWorkflowState::default();

    for line in text.lines() {
        if let Some(heading) = markdown_heading(line) {
            let normalized = heading.trim().trim_end_matches([':', '：']).trim();
            if is_completion_report_heading(normalized) {
                state.has_report = true;
                in_task_list = false;
                continue;
            }

            in_task_list = is_task_list_heading(normalized);
            saw_task_list |= in_task_list;
            continue;
        }

        if in_task_list && is_open_task_item(line) {
            state.has_open_tasks = true;
        }
    }

    saw_task_list.then_some(state)
}

fn is_task_list_heading(heading: &str) -> bool {
    heading.eq_ignore_ascii_case("任务列表")
        || heading.eq_ignore_ascii_case("任务清单")
        || heading.eq_ignore_ascii_case("task list")
        || heading.eq_ignore_ascii_case("tasks")
        || heading.eq_ignore_ascii_case("plan")
}

fn is_completion_report_heading(heading: &str) -> bool {
    heading.eq_ignore_ascii_case("完成报告")
        || heading.eq_ignore_ascii_case("completion report")
        || heading.eq_ignore_ascii_case("final report")
}

fn markdown_heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = trimmed.get(hashes..)?.trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.trim_end_matches('#').trim())
    }
}

fn is_open_task_item(line: &str) -> bool {
    let trimmed = line.trim_start();
    let item = if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        rest
    } else {
        let Some((prefix, rest)) = trimmed.split_once(' ') else {
            return false;
        };
        if !prefix
            .trim_end_matches(['.', ')'])
            .chars()
            .all(|ch| ch.is_ascii_digit())
        {
            return false;
        }
        rest
    };

    item.starts_with("[ ]") || item.starts_with("[]")
}

fn task_workflow_continue_message(run_id: &str, turn: usize) -> Message {
    Message {
        id: format!("{run_id}-task-workflow-continue-{turn}"),
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: "继续执行当前任务列表。不要只描述下一步；请调用合适的工具处理未完成项，并用 `developer__task_list_update` 更新进度。最终完成时调用 `developer__task_list_finish`。"
                .to_string(),
        }],
        created_at: Utc::now(),
    }
}

fn build_system_prompt(context: &RunContext, input_text: &str) -> String {
    let mut system = "You are Night24 Agent Core. When a task benefits from delegation, parallel analysis, background work, or isolated investigation, you may autonomously create and manage sub-agents using the developer__subagent_* tools. Prefer sub-agents for project analysis, repository or directory surveys, multi-file code reading, log/data review, long-context summarization, and any investigation likely to consume substantial context before the main task can proceed. Ask sub-agents to inspect independently and return concise summaries with key files, facts, risks, and recommended next actions, then use only that summary in the parent context. Use sync sub-agents when you need the result before continuing; use async sub-agents when work can continue while the child runs. Query the sub-agent pool before depending on asynchronous results.".to_string();
    system.push_str("\n\n");
    system.push_str(TASK_WORKFLOW_PROMPT);
    let skill_list = context.skills.available_for_prompt();
    if !skill_list.trim().is_empty() {
        system.push_str("\n\n");
        system.push_str(&skill_list);
        system.push_str(
            "\nUse developer__skill_load to load full skill instructions or bundle files when needed.",
        );
    }
    if let Some((_, skill)) = context.skills.explicit_invocation(input_text) {
        match context.skills.load_skill(&skill.name, None) {
            Ok(loaded) => {
                system.push_str("\n\nActive skill loaded by explicit user request:\n");
                system.push_str(&format!(
                    "Skill: {}\nDescription: {}\nInstructions:\n{}\n",
                    loaded.skill.name, loaded.skill.description, loaded.body
                ));
            }
            Err(err) => {
                system.push_str("\n\nExplicit skill request could not be loaded: ");
                system.push_str(&err.to_string());
            }
        }
    }
    system
}

async fn execute_skill_tool(
    context: &RunContext,
    arguments: &serde_json::Value,
) -> anyhow::Result<String> {
    let name = required_string_arg(arguments, "name")?;
    let file = string_arg(arguments, "file");
    let loaded = context.skills.load_skill(name, file.as_deref())?;
    Ok(serde_json::to_string_pretty(&loaded)?)
}

async fn execute_subagent_tool(
    params: &ReplyParams,
    default_provider: &str,
    context: &RunContext,
    tool_name: &str,
    arguments: &serde_json::Value,
) -> anyhow::Result<String> {
    match tool_name {
        "developer__subagent_spawn" => {
            spawn_subagent(params, default_provider, context, arguments).await
        }
        "developer__subagent_status" => {
            let id = string_arg(arguments, "subagent_id");
            let include_messages = bool_arg(arguments, "include_messages", false);
            let include_result = bool_arg(arguments, "include_result", false);
            let value =
                context
                    .subagents
                    .snapshot(id.as_deref(), None, include_messages, include_result)?;
            Ok(serde_json::to_string_pretty(&value)?)
        }
        "developer__subagent_message" => {
            let message = required_string_arg(arguments, "message")?;
            let target = string_arg(arguments, "subagent_id").or_else(|| context.agent_id.clone());
            let target = target.ok_or_else(|| {
                anyhow::anyhow!("subagent_id is required when parent sends a sub-agent message")
            })?;
            let direction = if context.agent_id.as_deref() == Some(target.as_str()) {
                SubAgentMessageDirection::ChildToParent
            } else {
                SubAgentMessageDirection::ParentToChild
            };
            let snapshot =
                context
                    .subagents
                    .add_message(&target, direction, message.to_string())?;
            Ok(serde_json::to_string_pretty(&snapshot)?)
        }
        "developer__subagent_wait" => {
            let id = required_string_arg(arguments, "subagent_id")?;
            let timeout_ms = u64_arg(arguments, "timeout_ms", 60_000);
            let include_messages = bool_arg(arguments, "include_messages", true);
            let snapshot = context
                .subagents
                .wait_for_terminal(id, timeout_ms, include_messages, true)
                .await?;
            Ok(serde_json::to_string_pretty(&snapshot)?)
        }
        "developer__subagent_cancel" => {
            let id = string_arg(arguments, "subagent_id");
            let snapshot = context.subagents.cancel(id.as_deref())?;
            Ok(serde_json::to_string_pretty(&snapshot)?)
        }
        _ => anyhow::bail!("unknown subagent tool: {tool_name}"),
    }
}

async fn spawn_subagent(
    params: &ReplyParams,
    default_provider: &str,
    context: &RunContext,
    arguments: &serde_json::Value,
) -> anyhow::Result<String> {
    let task = required_string_arg(arguments, "task")?.to_string();
    let mode = SubAgentMode::from_value(string_arg(arguments, "mode").as_deref());
    let name = string_arg(arguments, "name");
    let child_run_id = format!("{}:subagent:{}", context.run_id, uuid::Uuid::new_v4());
    let handle =
        context
            .subagents
            .create(&context.run_id, &params.session.id, &child_run_id, name.as_deref(), &task, mode)?;

    let child_params =
        build_child_reply_params(params, arguments, &handle.id, &child_run_id, &task);
    let child_context = RunContext {
        run_id: child_run_id,
        agent_id: Some(handle.id.clone()),
        emit_tool_events: params.options.emit_tool_events,
        cancelled: handle.cancelled.clone(),
        seq: Arc::new(AtomicU64::new(1)),
        output: context.output.clone(),
        collected: Some(Arc::new(Mutex::new(Vec::new()))),
        permissions: context.permissions.clone(),
        hooks: context.hooks.clone(),
        subagents: context.subagents.clone(),
        skills: context.skills.clone(),
        task_list: Arc::new(Mutex::new(TaskListState::default())),
    };

    let pool = context.subagents.clone();
    let default_provider = default_provider.to_string();
    let subagent_id = handle.id.clone();
    let parent_context = context.clone();
    let parent_session_id = params.session.id.clone();
    let child_run_id = child_params.run_id.clone();
    let subagent_name = name.unwrap_or_else(|| "subagent".to_string());
    send_subagent_session_event(
        &parent_context,
        &subagent_id,
        &child_run_id,
        &parent_session_id,
        &subagent_name,
        &task,
        "running",
        vec![Message::user(task.clone())],
    );

    match mode {
        SubAgentMode::Sync => {
            run_subagent_once(
                pool.clone(),
                subagent_id.clone(),
                child_params,
                default_provider,
                child_context,
                parent_context,
                parent_session_id,
                child_run_id,
                subagent_name,
                task.clone(),
            )
            .await;
            let snapshot = pool
                .wait_for_terminal(
                    &subagent_id,
                    u64_arg(arguments, "timeout_ms", 120_000),
                    true,
                    true,
                )
                .await?;
            Ok(serde_json::to_string_pretty(&snapshot)?)
        }
        SubAgentMode::Async => {
            let worker_pool = pool.clone();
            let worker_subagent_id = subagent_id.clone();
            std::thread::Builder::new()
                .name(format!("night24-subagent-{subagent_id}"))
                .spawn(move || {
                    let runtime = match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(runtime) => runtime,
                        Err(err) => {
                            worker_pool.mark_failed(
                                &worker_subagent_id,
                                format!("failed to start subagent runtime: {err}"),
                                Vec::new(),
                            );
                            return;
                        }
                    };
                    runtime.block_on(run_subagent_once(
                        worker_pool,
                        worker_subagent_id,
                        child_params,
                        default_provider,
                        child_context,
                        parent_context,
                        parent_session_id,
                        child_run_id,
                        subagent_name,
                        task,
                    ));
                })
                .map_err(|err| anyhow::anyhow!("failed to spawn subagent thread: {err}"))?;
            let value = serde_json::json!({
                "accepted": true,
                "subagent_id": subagent_id,
                "status": "running",
                "pool": pool.snapshot(None, None, false, false)?,
            });
            Ok(serde_json::to_string_pretty(&value)?)
        }
    }
}

async fn run_subagent_once(
    pool: Arc<SubAgentPool>,
    subagent_id: String,
    child_params: ReplyParams,
    default_provider: String,
    child_context: RunContext,
    parent_context: RunContext,
    parent_session_id: String,
    child_run_id: String,
    name: String,
    task: String,
) {
    pool.mark_running(&subagent_id);
    let child_output = child_context.output.clone();
    let events = Box::pin(reply_events(child_params, default_provider, child_context)).await;
    forward_returned_terminal_event(&events, &child_output);

    match subagent_result_from_events(&events) {
        Ok(result) => {
            let messages = subagent_session_messages_from_events(&task, &events);
            pool.mark_completed(&subagent_id, result, events.clone());
            send_subagent_session_event(
                &parent_context,
                &subagent_id,
                &child_run_id,
                &parent_session_id,
                &name,
                &task,
                "completed",
                messages,
            );
        }
        Err(error) => {
            let mut messages = subagent_session_messages_from_events(&task, &events);
            if messages.len() == 1 {
                messages.push(Message::assistant(error.clone()));
            }
            pool.mark_failed(&subagent_id, error, events.clone());
            send_subagent_session_event(
                &parent_context,
                &subagent_id,
                &child_run_id,
                &parent_session_id,
                &name,
                &task,
                "failed",
                messages,
            );
        }
    }
}

fn forward_returned_terminal_event(
    events: &[String],
    output: &Option<tokio::sync::mpsc::UnboundedSender<String>>,
) {
    let Some(output) = output else {
        return;
    };
    let Some(event) = events.last() else {
        return;
    };
    if is_terminal_agent_event_json(event) {
        let _ = output.send(event.clone());
    }
}

fn is_terminal_agent_event_json(event: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(event) else {
        return false;
    };
    matches!(
        value["params"]["type"].as_str(),
        Some("finish" | "error")
    )
}

fn build_child_reply_params(
    params: &ReplyParams,
    arguments: &serde_json::Value,
    subagent_id: &str,
    child_run_id: &str,
    task: &str,
) -> ReplyParams {
    let mut child = params.clone();
    child.run_id = child_run_id.to_string();
    child.session.id = subagent_id.to_string();
    child.session.name = string_arg(arguments, "name").unwrap_or_else(|| "subagent".to_string());
    child.session.conversation = Vec::new();
    child.input.text = format!(
        "You are a Night24 sub-agent. Complete the delegated task independently and return a concise result.\n\nSub-agent id: {subagent_id}\nTask:\n{task}"
    );
    if let Some(max_turns) = usize_arg(arguments, "max_turns") {
        child.limits.max_turns = max_turns.max(1);
    }
    if let Some(timeout_ms) = optional_u64_arg(arguments, "timeout_ms") {
        child.limits.total_timeout_ms = timeout_ms.max(1);
    }
    if let Some(provider) = string_arg(arguments, "provider") {
        child.provider.provider = provider;
    }
    if let Some(model) = string_arg(arguments, "model") {
        child.provider.model = model;
    }
    child
}

#[allow(clippy::too_many_arguments)]
fn send_subagent_session_event(
    parent_context: &RunContext,
    subagent_id: &str,
    child_run_id: &str,
    parent_session_id: &str,
    name: &str,
    task: &str,
    status: &str,
    messages: Vec<Message>,
) {
    parent_context.send(AgentEventKind::SubAgentSession {
        subagent_id: subagent_id.to_string(),
        child_run_id: child_run_id.to_string(),
        parent_session_id: parent_session_id.to_string(),
        parent_run_id: parent_context.run_id.clone(),
        name: name.to_string(),
        task: task.to_string(),
        status: status.to_string(),
        messages,
    });
}

fn subagent_session_messages_from_events(task: &str, events: &[String]) -> Vec<Message> {
    let mut messages = vec![Message::user(task.to_string())];
    for event in events.iter().rev() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(event) else {
            continue;
        };
        if value["params"]["type"].as_str() != Some("finish") {
            continue;
        }
        let Some(finish_messages) = value["params"]["payload"]["messages"].as_array() else {
            break;
        };
        messages.extend(
            finish_messages
                .iter()
                .filter_map(|message| serde_json::from_value::<Message>(message.clone()).ok()),
        );
        break;
    }
    messages
}

fn subagent_result_from_events(events: &[String]) -> Result<String, String> {
    for event in events.iter().rev() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(event) else {
            continue;
        };
        let Some(params) = value.get("params") else {
            continue;
        };
        match params.get("type").and_then(|value| value.as_str()) {
            Some("finish") => {
                let status = params["payload"]["status"].as_str().unwrap_or("completed");
                if status == "cancelled" || status == "failed" {
                    return Err(status.to_string());
                }
                let text = params["payload"]["messages"]
                    .as_array()
                    .map(|messages| {
                        messages
                            .iter()
                            .filter_map(message_text_from_value)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| status.to_string());
                return Ok(text);
            }
            Some("error") => {
                return Err(params["payload"]["message"]
                    .as_str()
                    .unwrap_or("subagent failed")
                    .to_string());
            }
            _ => {}
        }
    }
    Err("subagent produced no terminal event".to_string())
}

fn message_text_from_value(message: &serde_json::Value) -> Option<String> {
    let role = message.get("role").and_then(|value| value.as_str())?;
    if role != "assistant" && role != "tool" {
        return None;
    }
    let text = message
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|block| {
            block
                .get("text")
                .and_then(|value| value.as_str())
                .or_else(|| block.get("content").and_then(|value| value.as_str()))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn summarize_subagent_tool(tool_name: &str, arguments: &serde_json::Value) -> String {
    match tool_name {
        "developer__subagent_spawn" => arguments
            .get("task")
            .and_then(|value| value.as_str())
            .map(|task| format!("Spawn sub-agent: {}", preview_text(task)))
            .unwrap_or_else(|| "Spawn sub-agent".to_string()),
        "developer__subagent_status" => "Inspect sub-agent pool".to_string(),
        "developer__subagent_message" => "Send sub-agent message".to_string(),
        "developer__subagent_wait" => "Wait for sub-agent".to_string(),
        "developer__subagent_cancel" => "Cancel sub-agent".to_string(),
        _ => format!("Call {tool_name}"),
    }
}

fn summarize_skill_tool(arguments: &serde_json::Value) -> String {
    arguments
        .get("name")
        .and_then(|value| value.as_str())
        .map(|name| format!("Load skill: {name}"))
        .unwrap_or_else(|| "Load skill".to_string())
}

fn required_string_arg<'a>(arguments: &'a serde_json::Value, key: &str) -> anyhow::Result<&'a str> {
    arguments
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing `{key}`"))
}

fn string_arg(arguments: &serde_json::Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn bool_arg(arguments: &serde_json::Value, key: &str, default: bool) -> bool {
    arguments
        .get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(default)
}

fn optional_u64_arg(arguments: &serde_json::Value, key: &str) -> Option<u64> {
    arguments.get(key).and_then(|value| value.as_u64())
}

fn u64_arg(arguments: &serde_json::Value, key: &str, default: u64) -> u64 {
    optional_u64_arg(arguments, key).unwrap_or(default)
}

fn usize_arg(arguments: &serde_json::Value, key: &str) -> Option<usize> {
    arguments
        .get(key)
        .and_then(|value| value.as_u64())
        .and_then(|value| usize::try_from(value).ok())
}

fn preview_text(text: &str) -> String {
    const MAX_PREVIEW: usize = 500;
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= MAX_PREVIEW {
        compact
    } else {
        compact.chars().take(MAX_PREVIEW).collect::<String>() + "..."
    }
}

async fn reply_events(
    params: ReplyParams,
    default_provider: String,
    context: RunContext,
) -> Vec<String> {
    run_started_hook(&params, &default_provider, &context).await;
    let result = AgentCore::run_agent_with_events(&params, &default_provider, &context).await;
    finish_reply_events(params, default_provider, context, result).await
}

async fn run_started_hook(params: &ReplyParams, default_provider: &str, context: &RunContext) {
    let meta = run_lifecycle_hook_meta(params, default_provider);
    context
        .run_hooks(HookContext {
            event: HookEvent::RunStarted,
            run_id: &params.run_id,
            working_dir: &params.session.working_dir,
            provider: Some(&meta.provider),
            model: Some(&meta.model),
            message_count: Some(meta.message_count),
            tool_count: Some(meta.tool_count),
            tool_call_id: None,
            tool_name: None,
            summary: None,
            arguments: None,
            result_preview: None,
            error: None,
            duration_ms: None,
            finish_status: None,
        })
        .await;
}

async fn finish_reply_events(
    params: ReplyParams,
    default_provider: String,
    context: RunContext,
    result: anyhow::Result<Vec<Message>>,
) -> Vec<String> {
    match result {
        Ok(messages) => {
            let status = if context.is_cancelled() {
                "cancelled"
            } else {
                "completed"
            };
            run_end_hook(
                &params,
                &default_provider,
                &context,
                HookEvent::RunFinished,
                status,
                None,
            )
            .await;
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
            let is_cancelled = is_cancelled_error(&context, &message);
            let status = if is_cancelled { "cancelled" } else { "failed" };
            let event = if status == "failed" {
                HookEvent::RunFailed
            } else {
                HookEvent::RunFinished
            };
            run_end_hook(
                &params,
                &default_provider,
                &context,
                event,
                status,
                Some(&message),
            )
            .await;
            let mut output = context.drain_collected();
            if is_cancelled {
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

async fn run_end_hook(
    params: &ReplyParams,
    default_provider: &str,
    context: &RunContext,
    event: HookEvent,
    status: &str,
    error: Option<&str>,
) {
    let meta = run_lifecycle_hook_meta(params, default_provider);
    context
        .run_hooks(HookContext {
            event,
            run_id: &params.run_id,
            working_dir: &params.session.working_dir,
            provider: Some(&meta.provider),
            model: Some(&meta.model),
            message_count: Some(meta.message_count),
            tool_count: Some(meta.tool_count),
            tool_call_id: None,
            tool_name: None,
            summary: None,
            arguments: None,
            result_preview: None,
            error,
            duration_ms: None,
            finish_status: Some(status),
        })
        .await;
}

struct RunLifecycleHookMeta {
    provider: String,
    model: String,
    message_count: usize,
    tool_count: usize,
}

fn run_lifecycle_hook_meta(params: &ReplyParams, default_provider: &str) -> RunLifecycleHookMeta {
    let provider = effective_provider(&params.provider, default_provider);
    let model = effective_model(&provider);
    RunLifecycleHookMeta {
        provider: provider.provider,
        model,
        message_count: params.session.conversation.len() + 1,
        tool_count: night24_core::tool_executor::builtin_tools().len(),
    }
}

fn is_cancelled_error(context: &RunContext, message: &str) -> bool {
    context.is_cancelled() || message.contains("cancelled")
}

#[cfg(test)]
mod tests;
