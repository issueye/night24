use std::convert::Infallible;
use std::fmt::Display;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use futures::stream;
use tokio::sync::mpsc;
use tracing::info;

use night24_core::{
    agent::{Agent, AgentConfig},
    context_mgmt::{CompactionResult, ContextManager},
    model::{ContentBlock, Message, Role},
    provider::ModelConfig,
    session::{Session, SessionType},
};
use night24_protocol::{
    normalize_permission_mode, ProviderConfig, ReplyInput, ReplyLimits, ReplyOptions, ReplyParams,
    ReplySession,
};

use crate::api_types::{ReplyRequest, WorkspaceChangeSnapshot};
use crate::state::AppState;
use crate::workspace::{build_diff_ready_event, current_workspace_path, workspace_change_snapshot};

pub(crate) async fn reply_core(
    State(state): State<AppState>,
    Json(req): Json<ReplyRequest>,
) -> Response {
    let core_client = match state.core_client.read().await.clone() {
        Some(core_client) => core_client,
        None => {
            return sse_error_response(None, "core_unavailable", "no active core client", true);
        }
    };

    let prepared = match prepare_reply_session(&state, req).await {
        Ok(prepared) => prepared,
        Err(response) => return response,
    };
    let PreparedReplySession {
        session,
        run_id,
        user_message,
        reply_params,
    } = prepared;

    let (_accepted, core_events) = match core_client.reply(reply_params).await {
        Ok(value) => value,
        Err(err) => {
            return sse_error_response(Some(run_id), "core_reply_failed", err.to_string(), true);
        }
    };

    let (tx, rx) = mpsc::channel::<Result<String, Infallible>>(64);
    let session_manager = state.session_manager.clone();
    let diff_root = session.working_dir.clone();
    let diff_baseline = workspace_change_snapshot(&diff_root).ok();
    tokio::spawn(async move {
        let session_for_task = pump_core_events(
            core_events,
            tx,
            CoreEventPumpState {
                session,
                run_id,
                user_message,
                diff_root,
                diff_baseline,
            },
        )
        .await;
        let _ = session_manager.save(&session_for_task).await;
    });

    let stream = stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });

    sse_stream_response(Body::from_stream(stream))
}

struct PreparedReplySession {
    session: Session,
    run_id: String,
    user_message: Message,
    reply_params: ReplyParams,
}

struct CoreEventPumpState {
    session: Session,
    run_id: String,
    user_message: Message,
    diff_root: PathBuf,
    diff_baseline: Option<WorkspaceChangeSnapshot>,
}

struct CoreEventDispatch {
    events: Vec<serde_json::Value>,
    is_terminal: bool,
}

async fn pump_core_events(
    mut core_events: mpsc::Receiver<serde_json::Value>,
    tx: mpsc::Sender<Result<String, Infallible>>,
    mut state: CoreEventPumpState,
) -> Session {
    let user_message_id = state.user_message.id.clone();
    state.session.conversation.push(state.user_message.clone());

    while let Some(event) = core_events.recv().await {
        let dispatch = prepare_core_event_dispatch(&mut state, event);
        for event_to_send in dispatch.events {
            if tx.send(Ok(sse_format_event(&event_to_send))).await.is_err() {
                return finalize_pumped_session(state.session, &state.run_id, &user_message_id);
            }
        }
        if dispatch.is_terminal {
            break;
        }
    }

    finalize_pumped_session(state.session, &state.run_id, &user_message_id)
}

fn prepare_core_event_dispatch(
    state: &mut CoreEventPumpState,
    event: serde_json::Value,
) -> CoreEventDispatch {
    persist_core_event(&mut state.session.conversation, &event);

    let is_terminal = is_terminal_core_event(&event);
    let mut event_to_send = event;
    let mut events = Vec::with_capacity(if is_terminal { 2 } else { 1 });

    if is_terminal {
        let seq = core_event_seq(&event_to_send);
        let diff_event = build_diff_ready_event(
            &state.run_id,
            seq,
            &state.diff_root,
            state.diff_baseline.as_ref(),
        );
        append_diff_ready_before_terminal(&mut events, &mut event_to_send, diff_event);
    }

    events.push(event_to_send);
    CoreEventDispatch {
        events,
        is_terminal,
    }
}

fn append_diff_ready_before_terminal(
    events: &mut Vec<serde_json::Value>,
    terminal_event: &mut serde_json::Value,
    diff_event: Option<serde_json::Value>,
) {
    let Some(diff_event) = diff_event else {
        return;
    };
    set_core_event_seq(terminal_event, core_event_seq(terminal_event) + 1);
    events.push(diff_event);
}

fn finalize_pumped_session(mut session: Session, run_id: &str, user_message_id: &str) -> Session {
    if !conversation_has_assistant_after_current_user(&session.conversation, user_message_id) {
        session.conversation.push(text_message(
            Role::Assistant,
            format!("Run {run_id} completed without assistant message."),
        ));
    }

    if session.name == "session" || session.name.is_empty() {
        let derived = session.derived_name();
        if derived != session.name {
            session.rename(derived);
        }
    }

    session
}

async fn prepare_reply_session(
    state: &AppState,
    req: ReplyRequest,
) -> Result<PreparedReplySession, Response> {
    let mut session = load_or_create_reply_session(state, req.session_id.as_deref()).await?;
    compact_session_for_reply(&mut session, req.context_threshold_tokens);

    let run_id = format!("run-{}", uuid::Uuid::new_v4());
    let user_message = user_message(&req.text);
    let permission_mode = effective_permission_mode(req.permission_mode.clone());
    info!(
        run_id = %run_id,
        permission_mode = %permission_mode,
        "reply permission mode"
    );
    let reply_params = build_reply_params(&run_id, &session, req, permission_mode);

    Ok(PreparedReplySession {
        session,
        run_id,
        user_message,
        reply_params,
    })
}

async fn load_or_create_reply_session(
    state: &AppState,
    session_id: Option<&str>,
) -> Result<Session, Response> {
    if let Some(session_id) = session_id {
        return match state.session_manager.get(session_id).await {
            Ok(Some(existing)) => Ok(existing),
            Ok(None) => Err(session_not_found_response(session_id)),
            Err(err) => Err(session_operation_error_response("load", err)),
        };
    }

    let working_dir = current_workspace_path(state)
        .await
        .unwrap_or_else(|| PathBuf::from("."));
    state
        .session_manager
        .create("session", working_dir, SessionType::User)
        .await
        .map_err(|err| session_operation_error_response("create", err))
}

fn compact_session_for_reply(session: &mut Session, threshold: Option<usize>) {
    let Some(threshold) = threshold.filter(|value| *value > 0) else {
        return;
    };

    let context_manager = ContextManager::default();
    let compaction =
        context_manager.maybe_compact_by_token_threshold(&mut session.conversation, threshold);
    if let CompactionResult::Compacted { removed, current } = compaction {
        session.updated_at = chrono::Utc::now();
        info!(
            session_id = %session.id,
            threshold,
            removed,
            current,
            "session context summarized"
        );
    }
}

fn effective_permission_mode(request_mode: Option<String>) -> String {
    let mode = request_mode.or_else(|| std::env::var("NIGHT24_PERMISSION_MODE").ok());
    normalize_permission_mode(mode.as_deref())
}

fn user_message(text: &str) -> Message {
    text_message(Role::User, text.to_string())
}

fn text_message(role: Role, text: String) -> Message {
    Message {
        id: uuid::Uuid::new_v4().to_string(),
        role,
        content: vec![ContentBlock::Text { text }],
        created_at: chrono::Utc::now(),
    }
}

fn build_reply_params(
    run_id: &str,
    session: &Session,
    req: ReplyRequest,
    permission_mode: String,
) -> ReplyParams {
    let provider_name = normalize_provider_name(req.provider.as_deref());
    ReplyParams {
        run_id: run_id.to_string(),
        session: ReplySession {
            id: session.id.clone(),
            name: session.name.clone(),
            working_dir: session.working_dir.clone(),
            conversation: session.conversation.clone(),
        },
        input: ReplyInput { text: req.text },
        provider: ProviderConfig {
            provider: provider_name,
            model: req.model.unwrap_or_else(|| "echo-v1".to_string()),
            base_url: req.base_url,
            api_key_ref: None,
            api_key: req.api_key,
        },
        limits: ReplyLimits::default(),
        options: ReplyOptions {
            stream_message_delta: true,
            emit_tool_events: true,
            permission_mode: Some(permission_mode),
            network_proxy: normalize_network_proxy(req.network_proxy),
            context_threshold_tokens: req.context_threshold_tokens,
        },
    }
}

fn normalize_provider_name(value: Option<&str>) -> String {
    let Some(provider) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return "echo".to_string();
    };
    provider.to_ascii_lowercase()
}

fn normalize_network_proxy(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn persist_core_event(conversation: &mut Vec<Message>, event: &serde_json::Value) {
    let event_type = core_event_type(event);
    let Some(payload) = event.get("payload") else {
        return;
    };

    match event_type {
        "message" => {
            if let Some(message) = payload_message(payload) {
                merge_conversation_message(conversation, message);
            }
        }
        "message_delta" => {
            let Some(message_id) = payload.get("message_id").and_then(|value| value.as_str())
            else {
                return;
            };
            let delta = payload
                .get("delta")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if !delta.is_empty() {
                apply_message_delta(
                    conversation,
                    message_id,
                    delta,
                    event_created_at(event).unwrap_or_else(Utc::now),
                );
            }
        }
        "finish" => {
            for message in payload_messages(payload) {
                merge_conversation_message(conversation, message);
            }
        }
        _ => {}
    }
}

fn payload_message(payload: &serde_json::Value) -> Option<Message> {
    parse_message_value(payload.get("message")?)
}

fn payload_messages(payload: &serde_json::Value) -> Vec<Message> {
    payload
        .get("messages")
        .and_then(|value| value.as_array())
        .map(|messages| messages.iter().filter_map(parse_message_value).collect())
        .unwrap_or_default()
}

fn parse_message_value(value: &serde_json::Value) -> Option<Message> {
    serde_json::from_value::<Message>(value.clone()).ok()
}

fn merge_conversation_message(conversation: &mut Vec<Message>, message: Message) {
    if let Some(existing) = conversation
        .iter_mut()
        .find(|existing| !message.id.is_empty() && existing.id == message.id)
    {
        *existing = message;
    } else {
        conversation.push(message);
    }
}

fn apply_message_delta(
    conversation: &mut Vec<Message>,
    message_id: &str,
    delta: &str,
    created_at: DateTime<Utc>,
) {
    if let Some(message) = conversation
        .iter_mut()
        .find(|message| message.id == message_id)
    {
        append_text_delta(message, delta);
        return;
    }

    conversation.push(Message {
        id: message_id.to_string(),
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: delta.to_string(),
        }],
        created_at,
    });
}

fn append_text_delta(message: &mut Message, delta: &str) {
    if let Some(ContentBlock::Text { text }) = message
        .content
        .iter_mut()
        .find(|block| matches!(block, ContentBlock::Text { .. }))
    {
        text.push_str(delta);
    } else {
        message.content.insert(
            0,
            ContentBlock::Text {
                text: delta.to_string(),
            },
        );
    }
}

fn event_created_at(event: &serde_json::Value) -> Option<DateTime<Utc>> {
    event
        .get("created_at")
        .and_then(|value| value.as_str())
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn core_event_type(event: &serde_json::Value) -> &str {
    event
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("message")
}

fn is_terminal_core_event(event: &serde_json::Value) -> bool {
    matches!(core_event_type(event), "finish" | "error")
}

fn core_event_seq(event: &serde_json::Value) -> u64 {
    event
        .get("seq")
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
}

fn set_core_event_seq(event: &mut serde_json::Value, seq: u64) {
    if let Some(object) = event.as_object_mut() {
        object.insert("seq".to_string(), serde_json::json!(seq));
    }
}

fn conversation_has_assistant_after_current_user(conversation: &[Message], user_id: &str) -> bool {
    let Some(user_index) = conversation
        .iter()
        .position(|message| message.id == user_id)
    else {
        return conversation
            .iter()
            .any(|message| message.role == Role::Assistant);
    };
    conversation
        .iter()
        .skip(user_index + 1)
        .any(|message| message.role == Role::Assistant)
}

pub(crate) fn sse_format_event(event: &serde_json::Value) -> String {
    let event_type = core_event_type(event);
    format!("event: {event_type}\ndata: {event}\n\n")
}

fn sse_error_response(
    run_id: Option<String>,
    code: impl Into<String>,
    message: impl Into<String>,
    recoverable: bool,
) -> Response {
    let event = serde_json::json!({
        "type": "error",
        "run_id": run_id,
        "seq": null,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "payload": {
            "code": code.into(),
            "message": message.into(),
            "recoverable": recoverable
        }
    });
    let stream =
        stream::once(
            async move { Ok::<String, std::convert::Infallible>(sse_format_event(&event)) },
        );

    sse_stream_response(Body::from_stream(stream))
}

fn session_not_found_response(session_id: &str) -> Response {
    json_error_response(
        StatusCode::BAD_REQUEST,
        format!("session not found: {session_id}"),
    )
}

fn session_operation_error_response(action: &str, err: impl Display) -> Response {
    json_error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("failed to {action} session: {err}"),
    )
}

fn json_error_response(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}

fn sse_stream_response(body: Body) -> Response {
    Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(body)
        .unwrap()
        .into_response()
}

#[utoipa::path(
    post,
    path = "/reply",
    tag = "night24",
    request_body = ReplyRequest,
    responses(
        (status = 200, description = "SSE stream of agent messages", content_type = "text/event-stream")
    )
)]
#[allow(dead_code)]
pub(crate) async fn reply(
    State(state): State<AppState>,
    Json(req): Json<ReplyRequest>,
) -> Response {
    let provider_name = normalize_provider_name(req.provider.as_deref());
    let provider: Arc<dyn night24_core::provider::Provider> = if provider_name == "openai" {
        let api_key = req
            .api_key
            .unwrap_or_else(|| std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| "".to_string()));
        if api_key.is_empty() {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "api_key is required for openai provider"})),
            )
                .into_response();
        }
        state.provider_registry.create_with_model(
            "openai",
            req.model
                .clone()
                .unwrap_or_else(|| "gpt-4o-mini".to_string()),
        )
    } else if provider_name == "anthropic" {
        state.provider_registry.create_with_model(
            "anthropic",
            req.model
                .clone()
                .unwrap_or_else(|| "step-3.7-flash".to_string()),
        )
    } else if provider_name == "ollama" {
        state.provider_registry.create_with_model(
            "ollama",
            req.model.clone().unwrap_or_else(|| "llama3.2".to_string()),
        )
    } else if provider_name == "stepfun" {
        state.provider_registry.create_with_model(
            "stepfun",
            req.model
                .clone()
                .unwrap_or_else(|| "step-3.7-flash".to_string()),
        )
    } else if provider_name == "echo" {
        state.provider_registry.create("echo")
    } else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("unknown provider: {}", provider_name)})),
        )
            .into_response();
    };

    let session = if let Some(session_id) = req.session_id {
        match state.session_manager.get(&session_id).await {
            Ok(Some(existing)) => existing,
            Ok(None) => {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(
                        serde_json::json!({"error": format!("session not found: {}", session_id)}),
                    ),
                )
                    .into_response();
            }
            Err(_) => {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "failed to load session"})),
                )
                    .into_response();
            }
        }
    } else {
        let working_dir = current_workspace_path(&state)
            .await
            .unwrap_or_else(|| PathBuf::from("."));
        state
            .session_manager
            .create("session", working_dir, SessionType::User)
            .await
            .expect("failed to create session")
    };

    let user_message = Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: Role::User,
        content: vec![ContentBlock::Text { text: req.text }],
        created_at: chrono::Utc::now(),
    };

    let agent = Agent::with_permission_manager(
        AgentConfig {
            model_config: ModelConfig {
                model: req.model.clone().unwrap_or_else(|| "echo-v1".to_string()),
                temperature: None,
                max_tokens: None,
            },
            system_prompt: "You are a helpful AI assistant.".to_string(),
            max_turns: 40,
            turn_timeout: Duration::from_secs(60),
            tool_timeout: Duration::from_secs(60),
            total_timeout: Duration::from_secs(600),
        },
        provider,
        state.permission_manager.clone(),
    );

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Message, String>>(32);

    let session_manager = state.session_manager.clone();
    tokio::spawn(async move {
        let mut session_for_task = session.clone();
        let result = agent.run(&mut session_for_task, user_message).await;
        match result {
            Ok(messages) => {
                for msg in messages {
                    if tx.send(Ok(msg)).await.is_err() {
                        break;
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(Err(format!("agent error: {}", e))).await;
            }
        }
        if session_for_task.name == "session" || session_for_task.name.is_empty() {
            let derived = session_for_task.derived_name();
            if derived != session_for_task.name {
                session_for_task.rename(derived);
            }
        }
        let _ = session_manager.save(&session_for_task).await;
    });

    let stream = stream::unfold((rx, false), |(mut rx, finish_sent)| async move {
        match rx.recv().await {
            Some(Ok(m)) => {
                let json = serde_json::to_string(&m).unwrap_or_default();
                Some((
                    Ok::<String, std::convert::Infallible>(format!("data: {}\n\n", json)),
                    (rx, finish_sent),
                ))
            }
            Some(Err(e)) => {
                let error = serde_json::json!({
                    "type": "error",
                    "run_id": null,
                    "seq": null,
                    "created_at": chrono::Utc::now().to_rfc3339(),
                    "payload": {
                        "code": "agent_error",
                        "message": e,
                        "recoverable": true
                    }
                });
                Some((
                    Ok::<String, std::convert::Infallible>(format!(
                        "event: error\ndata: {}\n\n",
                        error
                    )),
                    (rx, true),
                ))
            }
            None if !finish_sent => {
                let finish = serde_json::json!({
                    "type": "finish",
                    "run_id": null,
                    "seq": null,
                    "created_at": chrono::Utc::now().to_rfc3339(),
                    "payload": {"status": "completed"}
                });
                Some((
                    Ok::<String, std::convert::Infallible>(format!(
                        "event: finish\ndata: {}\n\n",
                        finish
                    )),
                    (rx, true),
                ))
            }
            None => None,
        }
    });

    sse_stream_response(Body::from_stream(stream))
}

#[cfg(test)]
mod tests {
    use super::*;
    use night24_core::{
        permission::PermissionManager, provider::registry::ProviderRegistry,
        session::SessionManager,
    };

    fn reply_request(text: &str) -> ReplyRequest {
        ReplyRequest {
            text: text.to_string(),
            provider: None,
            api_key: None,
            base_url: None,
            model: None,
            session_id: None,
            permission_mode: None,
            network_proxy: None,
            context_threshold_tokens: None,
        }
    }

    fn test_state_with_workspace(root_path: PathBuf) -> AppState {
        AppState {
            session_manager: Arc::new(SessionManager::new()),
            provider_registry: Arc::new(ProviderRegistry::new("echo").with_echo()),
            permission_manager: Arc::new(PermissionManager::default()),
            workspace_state: Arc::new(tokio::sync::RwLock::new(crate::api_types::WorkspaceState {
                current: Some(crate::api_types::WorkspaceInfo {
                    id: "workspace-test".to_string(),
                    name: "test".to_string(),
                    root_path: root_path.to_string_lossy().to_string(),
                    created_at: "2026-07-05T00:00:00Z".to_string(),
                    last_opened_at: "2026-07-05T00:00:00Z".to_string(),
                }),
                recent: Vec::new(),
            })),
            core_client: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    #[test]
    fn permission_mode_normalization() {
        assert_eq!(normalize_permission_mode(Some("allow_all")), "allow_all");
        assert_eq!(normalize_permission_mode(Some("allow-all")), "allow_all");
        assert_eq!(normalize_permission_mode(Some("full_access")), "allow_all");
        assert_eq!(normalize_permission_mode(Some("deny-all")), "deny_all");
        assert_eq!(normalize_permission_mode(Some("permissive")), "permissive");
        assert_eq!(normalize_permission_mode(Some("unknown")), "strict");
        assert_eq!(normalize_permission_mode(None), "strict");
    }

    #[test]
    fn network_proxy_normalization_trims_empty_values() {
        assert_eq!(normalize_network_proxy(None), None);
        assert_eq!(normalize_network_proxy(Some("   ".to_string())), None);
        assert_eq!(
            normalize_network_proxy(Some(" http://127.0.0.1:7890 ".to_string())),
            Some("http://127.0.0.1:7890".to_string())
        );
    }

    #[test]
    fn provider_name_normalization_trims_cases_and_defaults_blank_values() {
        assert_eq!(normalize_provider_name(None), "echo");
        assert_eq!(normalize_provider_name(Some("   ")), "echo");
        assert_eq!(normalize_provider_name(Some(" OpenAI ")), "openai");
        assert_eq!(normalize_provider_name(Some("STEPFUN")), "stepfun");
    }

    #[test]
    fn reply_params_use_request_provider_defaults() {
        let session = Session::new(
            "session",
            std::path::PathBuf::from("E:/codes/project"),
            SessionType::User,
        );
        let params = build_reply_params(
            "run-1",
            &session,
            ReplyRequest {
                text: "hello".to_string(),
                provider: None,
                api_key: None,
                base_url: None,
                model: None,
                session_id: Some(session.id.clone()),
                permission_mode: None,
                network_proxy: Some(" http://127.0.0.1:7890 ".to_string()),
                context_threshold_tokens: Some(24000),
            },
            "strict".to_string(),
        );

        assert_eq!(params.run_id, "run-1");
        assert_eq!(params.input.text, "hello");
        assert_eq!(params.provider.provider, "echo");
        assert_eq!(params.provider.model, "echo-v1");
        assert_eq!(
            params.options.network_proxy.as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(params.options.context_threshold_tokens, Some(24000));
        assert_eq!(params.options.permission_mode.as_deref(), Some("strict"));
    }

    #[test]
    fn reply_params_normalize_request_provider_name() {
        let session = Session::new(
            "session",
            std::path::PathBuf::from("E:/codes/project"),
            SessionType::User,
        );
        let params = build_reply_params(
            "run-1",
            &session,
            ReplyRequest {
                text: "hello".to_string(),
                provider: Some(" StepFun ".to_string()),
                api_key: None,
                base_url: None,
                model: None,
                session_id: Some(session.id.clone()),
                permission_mode: None,
                network_proxy: None,
                context_threshold_tokens: None,
            },
            "strict".to_string(),
        );

        assert_eq!(params.provider.provider, "stepfun");
    }

    #[test]
    fn sse_stream_response_sets_standard_headers() {
        let response = sse_stream_response(Body::empty());

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-cache"
        );
        assert_eq!(
            response.headers().get(header::CONNECTION).unwrap(),
            "keep-alive"
        );
    }

    #[tokio::test]
    async fn json_error_response_sets_status_and_body() {
        let response = json_error_response(axum::http::StatusCode::BAD_REQUEST, "not found");

        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        let value: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be json");
        assert_eq!(value, serde_json::json!({ "error": "not found" }));
    }

    #[tokio::test]
    async fn session_error_responses_keep_http_contract() {
        let not_found = session_not_found_response("session-1");
        assert_eq!(not_found.status(), axum::http::StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(not_found.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        let value: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be json");
        assert_eq!(
            value,
            serde_json::json!({ "error": "session not found: session-1" })
        );

        let failed = session_operation_error_response("load", "database unavailable");
        assert_eq!(
            failed.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        let body = axum::body::to_bytes(failed.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        let value: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be json");
        assert_eq!(
            value,
            serde_json::json!({ "error": "failed to load session: database unavailable" })
        );
    }

    #[tokio::test]
    async fn prepare_reply_session_creates_session_from_current_workspace() {
        let workspace_root = PathBuf::from("E:/codes/project");
        let state = test_state_with_workspace(workspace_root.clone());
        let prepared = prepare_reply_session(&state, reply_request("hello"))
            .await
            .expect("reply session should be prepared");

        assert!(prepared.run_id.starts_with("run-"));
        assert_eq!(prepared.session.name, "session");
        assert_eq!(prepared.session.working_dir, workspace_root);
        assert_eq!(prepared.reply_params.session.id, prepared.session.id);
        assert_eq!(prepared.reply_params.session.working_dir, workspace_root);
        assert_eq!(prepared.reply_params.input.text, "hello");
        assert_eq!(prepared.user_message.role, Role::User);
        match &prepared.user_message.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected text block"),
        }

        let saved = state
            .session_manager
            .get(&prepared.session.id)
            .await
            .expect("session lookup should succeed")
            .expect("created session should be saved");
        assert_eq!(saved.working_dir, workspace_root);
    }

    #[tokio::test]
    async fn prepare_reply_session_uses_existing_session_working_dir() {
        let workspace_root = PathBuf::from("E:/codes/current-workspace");
        let session_root = PathBuf::from("E:/codes/session-workspace");
        let state = test_state_with_workspace(workspace_root);
        let existing = state
            .session_manager
            .create("existing", session_root.clone(), SessionType::User)
            .await
            .expect("session should be created");
        let mut request = reply_request("continue");
        request.session_id = Some(existing.id.clone());

        let prepared = prepare_reply_session(&state, request)
            .await
            .expect("existing reply session should be prepared");

        assert_eq!(prepared.session.id, existing.id);
        assert_eq!(prepared.session.working_dir, session_root);
        assert_eq!(prepared.reply_params.session.working_dir, session_root);
    }

    #[test]
    fn persists_streamed_message_delta_for_history_reload() {
        let mut conversation = Vec::new();
        persist_core_event(
            &mut conversation,
            &serde_json::json!({
                "type": "message_delta",
                "created_at": "2026-07-03T01:02:03Z",
                "payload": {
                    "message_id": "assistant-1",
                    "delta": "你好"
                }
            }),
        );
        persist_core_event(
            &mut conversation,
            &serde_json::json!({
                "type": "message_delta",
                "created_at": "2026-07-03T01:02:04Z",
                "payload": {
                    "message_id": "assistant-1",
                    "delta": "，这是回复"
                }
            }),
        );

        assert_eq!(conversation.len(), 1);
        assert_eq!(conversation[0].role, Role::Assistant);
        match &conversation[0].content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "你好，这是回复"),
            _ => panic!("expected text block"),
        }
    }

    #[test]
    fn finish_messages_replace_streamed_partial_message() {
        let mut conversation = vec![Message {
            id: "assistant-1".to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "partial".to_string(),
            }],
            created_at: Utc::now(),
        }];

        persist_core_event(
            &mut conversation,
            &serde_json::json!({
                "type": "finish",
                "payload": {
                    "status": "completed",
                    "messages": [{
                        "id": "assistant-1",
                        "role": "assistant",
                        "content": [{ "type": "text", "text": "final reply" }],
                        "created_at": "2026-07-03T01:02:05Z"
                    }]
                }
            }),
        );

        assert_eq!(conversation.len(), 1);
        match &conversation[0].content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "final reply"),
            _ => panic!("expected text block"),
        }
    }

    #[test]
    fn payload_message_helpers_ignore_malformed_values() {
        let payload = serde_json::json!({
            "message": {
                "id": "assistant-1",
                "role": "assistant",
                "content": [{ "type": "text", "text": "hello" }],
                "created_at": "2026-07-03T01:02:03Z"
            }
        });
        assert_eq!(payload_message(&payload).unwrap().id, "assistant-1");

        let finish_payload = serde_json::json!({
            "messages": [
                {
                    "id": "assistant-2",
                    "role": "assistant",
                    "content": [{ "type": "text", "text": "final" }],
                    "created_at": "2026-07-03T01:02:04Z"
                },
                { "id": "broken", "content": [] }
            ]
        });
        let messages = payload_messages(&finish_payload);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, "assistant-2");

        assert!(payload_message(&serde_json::json!({})).is_none());
        assert!(payload_messages(&serde_json::json!({ "messages": "bad" })).is_empty());
    }

    #[test]
    fn core_event_terminal_and_seq_state_are_stable() {
        let missing_type = serde_json::json!({"payload": {}});
        let non_string_type = serde_json::json!({"type": 42, "payload": {}});
        let message = serde_json::json!({"type": "message", "seq": 7});
        let finish = serde_json::json!({"type": "finish", "seq": 12});
        let error_without_seq = serde_json::json!({"type": "error"});

        assert_eq!(core_event_type(&missing_type), "message");
        assert_eq!(core_event_type(&non_string_type), "message");
        assert!(!is_terminal_core_event(&missing_type));
        assert!(!is_terminal_core_event(&non_string_type));
        assert!(!is_terminal_core_event(&message));
        assert!(is_terminal_core_event(&finish));
        assert!(is_terminal_core_event(&error_without_seq));
        assert_eq!(core_event_seq(&message), 7);
        assert_eq!(core_event_seq(&finish), 12);
        assert_eq!(core_event_seq(&error_without_seq), 0);
    }

    #[test]
    fn diff_ready_helper_precedes_terminal_event_and_shifts_terminal_seq() {
        let mut events = Vec::new();
        let mut terminal = serde_json::json!({
            "type": "finish",
            "run_id": "run-1",
            "seq": 9
        });
        let diff = serde_json::json!({
            "type": "diff_ready",
            "run_id": "run-1",
            "seq": 9
        });

        append_diff_ready_before_terminal(&mut events, &mut terminal, Some(diff));

        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "diff_ready");
        assert_eq!(events[0]["seq"], 9);
        assert_eq!(terminal["seq"], 10);

        append_diff_ready_before_terminal(&mut events, &mut terminal, None);
        assert_eq!(events.len(), 1);
        assert_eq!(terminal["seq"], 10);
    }

    #[test]
    fn sse_format_event_defaults_non_string_type_to_message_event_name() {
        let event = serde_json::json!({
            "type": 42,
            "payload": { "message": "kept verbatim" }
        });

        assert_eq!(
            sse_format_event(&event),
            format!("event: message\ndata: {event}\n\n")
        );
    }

    #[test]
    fn assistant_placeholder_check_is_scoped_to_current_user() {
        let previous_assistant = Message::assistant("old reply");
        let current_user = Message::user("new question");
        let conversation = vec![previous_assistant, current_user.clone()];

        assert!(!conversation_has_assistant_after_current_user(
            &conversation,
            &current_user.id,
        ));
    }

    #[test]
    fn finalize_pumped_session_adds_placeholder_only_for_current_user_without_reply() {
        let previous_user = Message::user("old question");
        let previous_assistant = Message::assistant("old reply");
        let current_user = Message::user("new question");
        let session = Session {
            conversation: vec![
                previous_user,
                previous_assistant,
                current_user.clone(),
                Message::user("not an assistant"),
            ],
            ..Session::new(
                "session",
                PathBuf::from("E:/codes/project"),
                SessionType::User,
            )
        };

        let finalized = finalize_pumped_session(session, "run-test", &current_user.id);

        assert_eq!(finalized.conversation.len(), 5);
        let placeholder = finalized
            .conversation
            .last()
            .expect("placeholder should be appended");
        assert_eq!(placeholder.role, Role::Assistant);
        match &placeholder.content[0] {
            ContentBlock::Text { text } => {
                assert_eq!(text, "Run run-test completed without assistant message.")
            }
            _ => panic!("expected text block"),
        }
    }
}
