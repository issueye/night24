use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{header, Method, StatusCode},
    routing::delete,
    routing::get,
    routing::post,
    routing::put,
    Json, Router,
};
use tokio::signal;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

mod api_types;
mod auth;
mod core_client;
mod reply;
mod sessions;
mod state;
mod workspace;

use api_types::{
    AcceptedResponse, CancelAgentRequest, CoreStatusResponse, CreateSessionRequest,
    ForkSessionRequest, PermissionDecisionRequest, ReadyResponse, RenameSessionRequest,
    ReplyRequest, SessionSummary, ToolsResponse, WorkspaceState,
};
use auth::require_api_key;
use core_client::{AgentCoreClient, CoreRuntimeStatus};
use reply::reply_core;
use sessions::{
    create_session, delete_session, fork_session, get_session_history, list_sessions,
    rename_session,
};
use state::AppState;
use workspace::{
    current_workspace, open_workspace, recent_workspaces, workspace_diff, workspace_file,
    workspace_status, workspace_tree,
};

use night24_core::{
    provider::registry::ProviderRegistry, session::SessionManager, tool_executor::builtin_tools,
};
use night24_protocol::PermissionDecision;

fn session_database_url() -> String {
    std::env::var("NIGHT24_DATABASE_URL")
        .ok()
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(normalize_sqlite_database_url(trimmed))
            }
        })
        .unwrap_or_else(default_session_database_url)
}

fn default_session_database_url() -> String {
    let path = std::env::var("NIGHT24_DATA_DIR")
        .ok()
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(PathBuf::from(trimmed).join("night24.db"))
            }
        })
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("night24.db")
        });
    sqlite_file_url(path)
}

fn normalize_sqlite_database_url(value: &str) -> String {
    if value == "sqlite::memory:"
        || value.starts_with("sqlite:file:")
        || value.starts_with("sqlite://")
        || value.starts_with("sqlite:")
    {
        value.to_string()
    } else {
        sqlite_file_url(PathBuf::from(value))
    }
}

fn sqlite_file_url(path: PathBuf) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    format!("sqlite:file:{path}?mode=rwc")
}

#[derive(OpenApi)]
#[openapi(
    paths(
        healthz,
        reply::reply,
        sessions::list_sessions,
        sessions::get_session_history,
        sessions::create_session,
        sessions::rename_session,
        sessions::fork_session,
        workspace::workspace_status,
        workspace::workspace_diff
    ),
    components(schemas(ReplyRequest, CreateSessionRequest, RenameSessionRequest, ForkSessionRequest, SessionSummary)),
    tags(
        (name = "night24", description = "Night24 AI Agent API")
    )
)]
struct ApiDoc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!(
                    "{}=debug,tower_http=debug,axum=debug",
                    env!("CARGO_CRATE_NAME")
                )
                .into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!(cwd = %std::env::current_dir().unwrap_or_default().display(), db_url = %std::env::var("NIGHT24_DATABASE_URL").unwrap_or_default(), "starting night24-server");

    let db_url = session_database_url();
    info!(db_url, "initializing sqlite session store");
    let session_manager = match SessionManager::with_sqlite(&db_url).await {
        Ok(manager) => Arc::new(manager),
        Err(err) => {
            warn!(error = ?err, "failed to init sqlite session store, falling back to in-memory");
            Arc::new(SessionManager::new())
        }
    };
    let provider_registry = build_provider_registry();
    let permission_manager = build_permission_manager();
    let core_client = match AgentCoreClient::spawn().await {
        Ok(client) => {
            info!("agent-core initialized");
            Some(Arc::new(client))
        }
        Err(err) => {
            warn!(error = ?err, "agent-core unavailable; server will return recoverable core errors");
            None
        }
    };

    let app_state = AppState {
        session_manager,
        provider_registry: Arc::new(provider_registry),
        permission_manager: Arc::new(permission_manager),
        workspace_state: Arc::new(tokio::sync::RwLock::new(WorkspaceState::new())),
        core_client,
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/reply", post(reply_core))
        .route("/agent/cancel", post(agent_cancel))
        .route("/tools", get(get_tools))
        .route("/workspaces/open", post(open_workspace))
        .route("/workspaces/current", get(current_workspace))
        .route("/workspaces/recent", get(recent_workspaces))
        .route("/workspace/tree", get(workspace_tree))
        .route("/workspace/file", get(workspace_file))
        .route("/workspace/status", get(workspace_status))
        .route("/workspace/diff", get(workspace_diff))
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{id}", delete(delete_session))
        .route("/sessions/{id}/history", get(get_session_history))
        .route("/sessions/{id}/name", put(rename_session))
        .route("/sessions/{id}/fork", post(fork_session))
        .route("/permissions/{id}/approve", post(approve_permission))
        .route("/permissions/{id}/deny", post(deny_permission))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
                .allow_headers([
                    header::CONTENT_TYPE,
                    header::AUTHORIZATION,
                    header::HeaderName::from_static("x-api-key"),
                ]),
        );

    // When NIGHT24_API_KEY is set, require it on all routes except the public
    // documentation/health endpoints. When unset, the server is open (backwards
    // compatible with local development).
    let app = if let Ok(api_key) = std::env::var("NIGHT24_API_KEY") {
        if !api_key.is_empty() {
            info!("API key authentication enabled");
            app.layer(axum::middleware::from_fn_with_state(
                api_key,
                require_api_key,
            ))
        } else {
            app
        }
    } else {
        app
    };

    let app = app.with_state(app_state);

    let addr: SocketAddr = std::env::args()
        .nth(1)
        .filter(|a| a != "serve")
        .unwrap_or_else(|| "0.0.0.0:17787".to_string())
        .parse()?;

    info!(?addr, "starting night24-server");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Build the provider registry from environment variables.
///
/// Providers are registered only when their API key / base URL is available:
/// - `OPENAI_API_KEY`         → openai (also drives stepfun if STEPFUN_API_KEY set)
/// - `ANTHROPIC_API_KEY`      → anthropic
/// - `STEPFUN_API_KEY`        → stepfun (OpenAI-compatible endpoint)
/// - `OLLAMA_BASE_URL`        → ollama (defaults to http://localhost:11434)
///
/// No secrets are hard-coded: every key must come from the environment.
fn build_provider_registry() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new("echo").with_echo();

    if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
        if !api_key.is_empty() {
            let base_url = std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
            let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
            registry = registry.with_openai(api_key, base_url, model);
        }
    }

    if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
        if !api_key.is_empty() {
            let base_url = std::env::var("ANTHROPIC_BASE_URL")
                .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
            let model = std::env::var("ANTHROPIC_MODEL")
                .unwrap_or_else(|_| "claude-3-5-sonnet-latest".to_string());
            registry = registry.with_anthropic(api_key, base_url, model);
        }
    }

    if let Ok(api_key) = std::env::var("STEPFUN_API_KEY") {
        if !api_key.is_empty() {
            let base_url = std::env::var("STEPFUN_BASE_URL")
                .unwrap_or_else(|_| "https://api.stepfun.com/step_plan/v1".to_string());
            let model =
                std::env::var("STEPFUN_MODEL").unwrap_or_else(|_| "step-3.7-flash".to_string());
            registry = registry.with_stepfun(api_key, base_url, model);
        }
    }

    let ollama_base_url =
        std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let ollama_model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2".to_string());
    registry = registry.with_ollama(ollama_base_url, ollama_model);

    registry
}

/// Build the permission manager based on `NIGHT24_PERMISSION_MODE`:
/// - `strict`     (default): every tool requires confirmation
/// - `permissive`: read-only tools auto-allowed, shell/write still confirm
/// - `allow_all`:  every tool auto-allowed (development only)
/// - `deny_all`:   every tool denied
fn build_permission_manager() -> night24_core::permission::PermissionManager {
    use night24_core::permission::{PermissionLevel, PermissionManager};
    let mode = std::env::var("NIGHT24_PERMISSION_MODE")
        .unwrap_or_else(|_| "strict".to_string())
        .to_ascii_lowercase();
    match mode.as_str() {
        "permissive" => {
            info!(permission_mode = %mode, "permission: permissive (read-only allowed)");
            PermissionManager::permissive_local()
        }
        "allow_all" | "allow-all" => {
            info!(permission_mode = %mode, "permission: allow_all (NOT for production)");
            PermissionManager::new(PermissionLevel::Allow)
        }
        "deny_all" | "deny-all" => {
            info!(permission_mode = %mode, "permission: deny_all");
            PermissionManager::new(PermissionLevel::Deny)
        }
        _ => {
            info!(
                permission_mode = "strict",
                "permission: strict (confirm all)"
            );
            PermissionManager::default()
        }
    }
}

#[cfg(unix)]
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("signal received, starting graceful shutdown");
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    tokio::select! {
        _ = ctrl_c => {},
    }

    info!("signal received, starting graceful shutdown");
}

fn json_error(
    status: StatusCode,
    message: impl Into<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": message.into() })))
}

#[utoipa::path(
    get,
    path = "/healthz",
    tag = "night24",
    responses(
        (status = 200, description = "Service is healthy", body = str)
    )
)]
async fn healthz() -> &'static str {
    "ok"
}

#[utoipa::path(
    get,
    path = "/readyz",
    tag = "night24",
    responses(
        (status = 200, description = "Server readiness", body = ReadyResponse)
    )
)]
async fn readyz(State(state): State<AppState>) -> Json<ReadyResponse> {
    let core = match &state.core_client {
        Some(core_client) => core_client.status().await,
        None => CoreRuntimeStatus::unavailable("no active core client"),
    };
    Json(ReadyResponse {
        ready: core.available && core.initialized,
        server: "ok".to_string(),
        core: CoreStatusResponse {
            available: core.available,
            initialized: core.initialized,
            reason: core.reason,
        },
    })
}

#[utoipa::path(
    get,
    path = "/tools",
    tag = "night24",
    responses(
        (status = 200, description = "Available tool definitions", body = ToolsResponse)
    )
)]
async fn get_tools(State(state): State<AppState>) -> Json<ToolsResponse> {
    if let Some(core_client) = &state.core_client {
        match core_client.tools().await {
            Ok(result) => {
                return Json(ToolsResponse {
                    tools: result.tools,
                    source: "night24-agent-core".to_string(),
                    core_available: true,
                });
            }
            Err(err) => {
                warn!(error = ?err, "failed to fetch tools from agent-core");
            }
        }
    }
    Json(ToolsResponse {
        tools: builtin_tools(),
        source: "night24-core builtin fallback".to_string(),
        core_available: false,
    })
}

#[utoipa::path(
    post,
    path = "/agent/cancel",
    tag = "night24",
    request_body = CancelAgentRequest,
    responses(
        (status = 200, description = "Cancel request status", body = AcceptedResponse)
    )
)]
async fn agent_cancel(
    State(state): State<AppState>,
    Json(req): Json<CancelAgentRequest>,
) -> Json<AcceptedResponse> {
    if let (Some(core_client), Some(run_id)) = (&state.core_client, req.run_id.clone()) {
        match core_client.cancel(run_id.clone(), req.reason.clone()).await {
            Ok(_) => {
                return Json(AcceptedResponse {
                    accepted: true,
                    reason: None,
                    run_id: Some(run_id),
                    permission_id: None,
                });
            }
            Err(err) => {
                return Json(AcceptedResponse {
                    accepted: false,
                    reason: Some(err.to_string()),
                    run_id: Some(run_id),
                    permission_id: None,
                });
            }
        }
    }
    let reason = req
        .reason
        .map(|reason| format!("no active core client or run_id: {}", reason))
        .unwrap_or_else(|| "no active core client or run_id".to_string());
    Json(AcceptedResponse {
        accepted: false,
        reason: Some(reason),
        run_id: req.run_id,
        permission_id: None,
    })
}

#[utoipa::path(
    post,
    path = "/permissions/{id}/approve",
    tag = "night24",
    params(
        ("id" = String, Path, description = "Permission request ID")
    ),
    request_body = PermissionDecisionRequest,
    responses(
        (status = 200, description = "Permission decision status", body = AcceptedResponse)
    )
)]
async fn approve_permission(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<PermissionDecisionRequest>,
) -> Json<AcceptedResponse> {
    if let (Some(core_client), Some(run_id)) = (&state.core_client, req.run_id.clone()) {
        match core_client
            .resolve_permission(
                run_id.clone(),
                id.clone(),
                PermissionDecision::Approve,
                req.reason.clone(),
            )
            .await
        {
            Ok(_) => {
                return Json(AcceptedResponse {
                    accepted: true,
                    reason: None,
                    run_id: Some(run_id),
                    permission_id: Some(id),
                });
            }
            Err(err) => {
                return Json(AcceptedResponse {
                    accepted: false,
                    reason: Some(err.to_string()),
                    run_id: Some(run_id),
                    permission_id: Some(id),
                });
            }
        }
    }
    let detail = req
        .reason
        .unwrap_or_else(|| "no active core client or run_id".to_string());
    Json(AcceptedResponse {
        accepted: false,
        reason: Some(detail),
        run_id: req.run_id,
        permission_id: Some(id),
    })
}

#[utoipa::path(
    post,
    path = "/permissions/{id}/deny",
    tag = "night24",
    params(
        ("id" = String, Path, description = "Permission request ID")
    ),
    request_body = PermissionDecisionRequest,
    responses(
        (status = 200, description = "Permission decision status", body = AcceptedResponse)
    )
)]
async fn deny_permission(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<PermissionDecisionRequest>,
) -> Json<AcceptedResponse> {
    if let (Some(core_client), Some(run_id)) = (&state.core_client, req.run_id.clone()) {
        match core_client
            .resolve_permission(
                run_id.clone(),
                id.clone(),
                PermissionDecision::Deny,
                req.reason.clone(),
            )
            .await
        {
            Ok(_) => {
                return Json(AcceptedResponse {
                    accepted: true,
                    reason: None,
                    run_id: Some(run_id),
                    permission_id: Some(id),
                });
            }
            Err(err) => {
                return Json(AcceptedResponse {
                    accepted: false,
                    reason: Some(err.to_string()),
                    run_id: Some(run_id),
                    permission_id: Some(id),
                });
            }
        }
    }
    let detail = req
        .reason
        .unwrap_or_else(|| "no active core client or run_id".to_string());
    Json(AcceptedResponse {
        accepted: false,
        reason: Some(detail),
        run_id: req.run_id,
        permission_id: Some(id),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{
        diff_stats, is_binary_bytes, parse_git_status, should_skip_workspace_entry,
        workspace_relative_path,
    };

    #[test]
    fn test_workspace_skip_dirs() {
        assert!(should_skip_workspace_entry(&PathBuf::from("node_modules")));
        assert!(should_skip_workspace_entry(&PathBuf::from(".git")));
        assert!(!should_skip_workspace_entry(&PathBuf::from("src")));
    }

    #[test]
    fn test_binary_detection() {
        assert!(is_binary_bytes(b"abc\0def"));
        assert!(!is_binary_bytes(b"plain utf-8 text"));
    }

    #[test]
    fn test_workspace_relative_path() {
        let root = PathBuf::from("workspace");
        let file = root.join("src").join("main.rs");
        assert_eq!(workspace_relative_path(&root, &file), "src/main.rs");
        assert_eq!(workspace_relative_path(&root, &root), ".");
    }

    #[test]
    fn test_parse_git_status() {
        let files = parse_git_status(" M src/main.rs\nA  README.md\nR  old.txt -> new.txt\n");
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[0].index_status, " ");
        assert_eq!(files[0].worktree_status, "M");
        assert_eq!(files[2].path, "new.txt");
    }

    #[test]
    fn test_diff_stats() {
        let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@\n-old\n+new\n+line\n";
        let (files, insertions, deletions) = diff_stats(diff);
        assert_eq!(files, 1);
        assert_eq!(insertions, 2);
        assert_eq!(deletions, 1);
    }

    #[test]
    fn test_normalize_sqlite_database_url() {
        assert_eq!(
            normalize_sqlite_database_url("sqlite::memory:"),
            "sqlite::memory:"
        );
        assert_eq!(
            normalize_sqlite_database_url("sqlite:file:night24.db?mode=rwc"),
            "sqlite:file:night24.db?mode=rwc"
        );
        assert_eq!(
            normalize_sqlite_database_url("custom.db"),
            "sqlite:file:custom.db?mode=rwc"
        );
    }

    #[test]
    fn test_sqlite_file_url_normalizes_windows_separators() {
        assert_eq!(
            sqlite_file_url(PathBuf::from("data\\night24.db")),
            "sqlite:file:data/night24.db?mode=rwc"
        );
    }
}
