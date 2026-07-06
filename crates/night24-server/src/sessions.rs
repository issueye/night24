use std::path::PathBuf;

use axum::{
    extract::{Path, State},
    Json,
};

use night24_core::{
    context_mgmt::{CompactionResult, ContextManager},
    model::Message,
    session::{Session, SessionType},
};

use crate::api_types::{
    CompactSessionRequest, CompactSessionResponse, CreateSessionRequest, ForkSessionRequest,
    RenameSessionRequest, SessionSummary,
};
use crate::state::AppState;
use crate::workspace::current_workspace_path;

#[utoipa::path(
    get,
    path = "/sessions",
    tag = "night24",
    responses(
        (status = 200, description = "List all sessions", body = Vec<SessionSummary>)
    )
)]
pub(crate) async fn list_sessions(
    State(state): State<AppState>,
) -> Result<Json<Vec<SessionSummary>>, axum::http::StatusCode> {
    let sessions = state
        .session_manager
        .list()
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let summaries = sessions
        .into_iter()
        .map(|s| SessionSummary {
            id: s.id,
            name: s.name,
            session_type: format!("{:?}", s.session_type),
            working_dir: s.working_dir.to_string_lossy().to_string(),
            updated_at: s.updated_at.to_rfc3339(),
        })
        .collect();
    Ok(Json(summaries))
}

#[utoipa::path(
    delete,
    path = "/sessions/{id}",
    tag = "night24",
    params(
        ("id" = String, Path, description = "Session ID")
    ),
    responses(
        (status = 200, description = "Deleted session", body = serde_json::Value),
        (status = 404, description = "Session not found")
    )
)]
pub(crate) async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    match state.session_manager.delete(&id).await {
        Ok(true) => Ok(Json(serde_json::json!({"deleted": true, "id": id}))),
        Ok(false) => Err(axum::http::StatusCode::NOT_FOUND),
        Err(_) => Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[utoipa::path(
    post,
    path = "/sessions",
    tag = "night24",
    request_body = CreateSessionRequest,
    responses(
        (status = 200, description = "Created session", body = SessionSummary)
    )
)]
pub(crate) async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<Session>, axum::http::StatusCode> {
    let name = req.name.unwrap_or_else(|| "session".to_string());
    let working_dir = if let Some(path) = req.working_dir {
        PathBuf::from(path)
    } else {
        current_workspace_path(&state)
            .await
            .unwrap_or_else(|| PathBuf::from("."))
    };
    let session_type = parse_session_type(req.session_type.as_deref());
    let session = state
        .session_manager
        .create(name, working_dir, session_type)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(session))
}

#[utoipa::path(
    get,
    path = "/sessions/{id}/history",
    tag = "night24",
    params(
        ("id" = String, Path, description = "Session ID")
    ),
    responses(
        (status = 200, description = "Session conversation history", body = Vec<Message>)
    )
)]
pub(crate) async fn get_session_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Message>>, axum::http::StatusCode> {
    let session = state
        .session_manager
        .get(&id)
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    match session {
        Some(s) => Ok(Json(s.conversation)),
        None => Err(axum::http::StatusCode::NOT_FOUND),
    }
}

#[utoipa::path(
    put,
    path = "/sessions/{id}/name",
    tag = "night24",
    params(
        ("id" = String, Path, description = "Session ID")
    ),
    request_body = RenameSessionRequest,
    responses(
        (status = 200, description = "Renamed session", body = Session),
        (status = 404, description = "Session not found")
    )
)]
pub(crate) async fn rename_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<RenameSessionRequest>,
) -> Result<Json<Session>, (axum::http::StatusCode, String)> {
    match state.session_manager.rename(&id, req.name).await {
        Ok(session) => Ok(Json(session)),
        Err(err) => Err(session_mutation_error(err)),
    }
}

#[utoipa::path(
    post,
    path = "/sessions/{id}/fork",
    tag = "night24",
    params(
        ("id" = String, Path, description = "Source session ID")
    ),
    request_body = ForkSessionRequest,
    responses(
        (status = 200, description = "Forked session", body = Session),
        (status = 404, description = "Source session not found")
    )
)]
pub(crate) async fn fork_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ForkSessionRequest>,
) -> Result<Json<Session>, (axum::http::StatusCode, String)> {
    match state.session_manager.fork(&id, req.at_index).await {
        Ok(session) => Ok(Json(session)),
        Err(err) => Err(session_mutation_error(err)),
    }
}

#[utoipa::path(
    post,
    path = "/sessions/{id}/compact",
    tag = "night24",
    params(
        ("id" = String, Path, description = "Session ID")
    ),
    request_body = CompactSessionRequest,
    responses(
        (status = 200, description = "Compacted session conversation", body = CompactSessionResponse),
        (status = 404, description = "Session not found")
    )
)]
pub(crate) async fn compact_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CompactSessionRequest>,
) -> Result<Json<CompactSessionResponse>, (axum::http::StatusCode, String)> {
    let mut session = state
        .session_manager
        .get(&id)
        .await
        .map_err(|err| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                err.to_string(),
            )
        })?
        .ok_or_else(|| {
            (
                axum::http::StatusCode::NOT_FOUND,
                format!("session not found: {id}"),
            )
        })?;

    let mut manager = ContextManager::default();
    if req.force {
        if let Some(preserve_recent) = forced_compaction_preserve_recent(session.conversation.len())
        {
            manager.preserve_recent = preserve_recent;
        }
    }
    let result = if req.force {
        manager.maybe_compact_by_token_threshold(&mut session.conversation, 1)
    } else if let Some(threshold) = req.threshold_tokens.filter(|value| *value > 0) {
        manager.maybe_compact_by_token_threshold(&mut session.conversation, threshold)
    } else {
        manager.maybe_compact(&mut session.conversation)
    };
    let token_estimate = manager.estimate_tokens(&session.conversation);
    let (compacted, removed, current) = match result {
        CompactionResult::Noop => (false, 0, session.conversation.len()),
        CompactionResult::Compacted { removed, current } => {
            session.updated_at = chrono::Utc::now();
            state.session_manager.save(&session).await.map_err(|err| {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    err.to_string(),
                )
            })?;
            (true, removed, current)
        }
    };

    Ok(Json(CompactSessionResponse {
        compacted,
        removed,
        current,
        token_estimate,
        conversation: session.conversation,
    }))
}

fn session_mutation_error(error: impl ToString) -> (axum::http::StatusCode, String) {
    let message = error.to_string();
    let status = if message.contains("not found") {
        axum::http::StatusCode::NOT_FOUND
    } else {
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, message)
}

fn parse_session_type(value: Option<&str>) -> SessionType {
    match value.map(str::trim) {
        Some("scheduled") => SessionType::Scheduled,
        Some("sub_agent") => SessionType::SubAgent,
        Some("hidden") => SessionType::Hidden,
        Some("terminal") => SessionType::Terminal,
        Some("gateway") => SessionType::Gateway,
        Some("acp") => SessionType::Acp,
        _ => SessionType::User,
    }
}

fn forced_compaction_preserve_recent(conversation_len: usize) -> Option<usize> {
    if conversation_len > 1 {
        Some((conversation_len - 1).min(6).max(1))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_mutation_error_maps_not_found_messages() {
        let (status, message) = session_mutation_error("session not found: abc");

        assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
        assert_eq!(message, "session not found: abc");
    }

    #[test]
    fn session_mutation_error_keeps_internal_messages() {
        let (status, message) = session_mutation_error("storage unavailable");

        assert_eq!(status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(message, "storage unavailable");
    }

    #[test]
    fn parse_session_type_trims_and_defaults_unknown_values() {
        assert_eq!(
            format!("{:?}", parse_session_type(Some(" scheduled "))),
            "Scheduled"
        );
        assert_eq!(
            format!("{:?}", parse_session_type(Some("sub_agent"))),
            "SubAgent"
        );
        assert_eq!(
            format!("{:?}", parse_session_type(Some(" hidden\t"))),
            "Hidden"
        );
        assert_eq!(
            format!("{:?}", parse_session_type(Some("terminal"))),
            "Terminal"
        );
        assert_eq!(
            format!("{:?}", parse_session_type(Some("gateway"))),
            "Gateway"
        );
        assert_eq!(format!("{:?}", parse_session_type(Some("acp"))), "Acp");
        assert_eq!(format!("{:?}", parse_session_type(Some("unknown"))), "User");
        assert_eq!(format!("{:?}", parse_session_type(Some("   "))), "User");
        assert_eq!(format!("{:?}", parse_session_type(None)), "User");
    }

    #[test]
    fn forced_compaction_preserve_recent_keeps_expected_bounds() {
        assert_eq!(forced_compaction_preserve_recent(0), None);
        assert_eq!(forced_compaction_preserve_recent(1), None);
        assert_eq!(forced_compaction_preserve_recent(2), Some(1));
        assert_eq!(forced_compaction_preserve_recent(7), Some(6));
        assert_eq!(forced_compaction_preserve_recent(20), Some(6));
    }
}
