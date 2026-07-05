use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::State,
    http::header,
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
    session::SessionType,
};
use night24_protocol::{
    ProviderConfig, ReplyInput, ReplyLimits, ReplyOptions, ReplyParams, ReplySession,
};

use crate::api_types::ReplyRequest;
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

    let mut session = if let Some(session_id) = req.session_id.clone() {
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
            Err(err) => {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("failed to load session: {err}")})),
                )
                    .into_response();
            }
        }
    } else {
        let working_dir = current_workspace_path(&state)
            .await
            .unwrap_or_else(|| PathBuf::from("."));
        match state
            .session_manager
            .create("session", working_dir, SessionType::User)
            .await
        {
            Ok(session) => session,
            Err(err) => {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("failed to create session: {err}")})),
                )
                    .into_response();
            }
        }
    };

    if let Some(threshold) = req.context_threshold_tokens.filter(|value| *value > 0) {
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

    let run_id = format!("run-{}", uuid::Uuid::new_v4());
    let user_message = Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: req.text.clone(),
        }],
        created_at: chrono::Utc::now(),
    };
    let permission_mode = normalize_permission_mode(
        req.permission_mode
            .or_else(|| std::env::var("NIGHT24_PERMISSION_MODE").ok()),
    );
    info!(
        run_id = %run_id,
        permission_mode = %permission_mode,
        "reply permission mode"
    );
    let reply_params = ReplyParams {
        run_id: run_id.clone(),
        session: ReplySession {
            id: session.id.clone(),
            name: session.name.clone(),
            working_dir: session.working_dir.clone(),
            conversation: session.conversation.clone(),
        },
        input: ReplyInput { text: req.text },
        provider: ProviderConfig {
            provider: req.provider.unwrap_or_else(|| "echo".to_string()),
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
            network_proxy: req.network_proxy.and_then(|value| {
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            }),
            context_threshold_tokens: req.context_threshold_tokens,
        },
    };

    let (_accepted, mut core_events) = match core_client.reply(reply_params).await {
        Ok(value) => value,
        Err(err) => {
            return sse_error_response(Some(run_id), "core_reply_failed", err.to_string(), true);
        }
    };

    let (tx, rx) = mpsc::channel::<Result<String, std::convert::Infallible>>(64);
    let session_manager = state.session_manager.clone();
    let run_id_for_task = run_id.clone();
    let user_message_id = user_message.id.clone();
    let diff_root = session.working_dir.clone();
    let diff_baseline = workspace_change_snapshot(&diff_root).ok();
    tokio::spawn(async move {
        let mut session_for_task = session;
        session_for_task.conversation.push(user_message);

        while let Some(event) = core_events.recv().await {
            persist_core_event(&mut session_for_task.conversation, &event);

            let event_type = event
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or("message")
                .to_string();
            let is_terminal = event_type == "finish" || event_type == "error";

            let mut event_to_send = event.clone();
            if is_terminal {
                let seq = event
                    .get("seq")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0);
                if let Some(diff_event) = build_diff_ready_event(
                    &run_id_for_task,
                    seq,
                    &diff_root,
                    diff_baseline.as_ref(),
                ) {
                    if let Some(object) = event_to_send.as_object_mut() {
                        object.insert("seq".to_string(), serde_json::json!(seq + 1));
                    }
                    if tx.send(Ok(sse_format_event(&diff_event))).await.is_err() {
                        break;
                    }
                }
            }

            if tx.send(Ok(sse_format_event(&event_to_send))).await.is_err() {
                break;
            }
            if is_terminal {
                break;
            }
        }

        if !conversation_has_assistant_after_current_user(
            &session_for_task.conversation,
            &user_message_id,
        ) {
            session_for_task.conversation.push(Message {
                id: uuid::Uuid::new_v4().to_string(),
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: format!("Run {run_id_for_task} completed without assistant message."),
                }],
                created_at: chrono::Utc::now(),
            });
        }

        if session_for_task.name == "session" || session_for_task.name.is_empty() {
            let derived = session_for_task.derived_name();
            if derived != session_for_task.name {
                session_for_task.rename(derived);
            }
        }
        let _ = session_manager.save(&session_for_task).await;
    });

    let stream = stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });

    Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(Body::from_stream(stream))
        .unwrap()
        .into_response()
}

pub(crate) fn normalize_permission_mode(mode: Option<String>) -> String {
    match mode
        .unwrap_or_else(|| "strict".to_string())
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .as_str()
    {
        "allow_all" | "full_access" => "allow_all".to_string(),
        "permissive" => "permissive".to_string(),
        "deny_all" => "deny_all".to_string(),
        _ => "strict".to_string(),
    }
}

fn persist_core_event(conversation: &mut Vec<Message>, event: &serde_json::Value) {
    let event_type = event
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("message");
    let Some(payload) = event.get("payload") else {
        return;
    };

    match event_type {
        "message" => {
            if let Some(message) = payload
                .get("message")
                .cloned()
                .and_then(|value| serde_json::from_value::<Message>(value).ok())
            {
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
            if let Some(messages) = payload.get("messages").and_then(|value| value.as_array()) {
                for message in messages {
                    if let Ok(message) = serde_json::from_value::<Message>(message.clone()) {
                        merge_conversation_message(conversation, message);
                    }
                }
            }
        }
        _ => {}
    }
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
    let event_type = event
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("message");
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

    Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(Body::from_stream(stream))
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
    let provider_name = req.provider.as_deref().unwrap_or("echo");
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

    Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(Body::from_stream(stream))
        .unwrap()
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_mode_normalization() {
        assert_eq!(
            normalize_permission_mode(Some("allow_all".to_string())),
            "allow_all"
        );
        assert_eq!(
            normalize_permission_mode(Some("allow-all".to_string())),
            "allow_all"
        );
        assert_eq!(
            normalize_permission_mode(Some("full_access".to_string())),
            "allow_all"
        );
        assert_eq!(
            normalize_permission_mode(Some("permissive".to_string())),
            "permissive"
        );
        assert_eq!(
            normalize_permission_mode(Some("unknown".to_string())),
            "strict"
        );
        assert_eq!(normalize_permission_mode(None), "strict");
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
    fn assistant_placeholder_check_is_scoped_to_current_user() {
        let previous_assistant = Message::assistant("old reply");
        let current_user = Message::user("new question");
        let conversation = vec![previous_assistant, current_user.clone()];

        assert!(!conversation_has_assistant_after_current_user(
            &conversation,
            &current_user.id,
        ));
    }
}
