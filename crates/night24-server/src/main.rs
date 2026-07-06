use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
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

mod agent_runner;
mod api_types;
mod auth;
mod core_client;
mod hooks;
mod reply;
mod run_events;
mod sessions;
mod state;
mod workspace;

use agent_runner::{build_agent_runner, AgentRunner, RunnerMode};
use api_types::{
    AcceptedResponse, CancelAgentRequest, CompactSessionRequest, CompactSessionResponse,
    CoreRestartResponse, CoreStatusResponse, CreateSessionRequest, ForkSessionRequest,
    PermissionDecisionRequest, ReadyResponse, RenameSessionRequest, ReplyRequest, SessionSummary,
    SkillLoadQuery, SubAgentPoolQuery, ToolsResponse, WorkspaceState,
};
use auth::require_api_key;
use core_client::{AgentCoreClient, CoreRuntimeStatus};
use hooks::{get_workspace_hooks, put_workspace_hooks};
use reply::{reply_core, stream_run_events};
use run_events::RunEventStore;
use sessions::{
    compact_session, create_session, delete_session, fork_session, get_session_history,
    list_sessions, rename_session,
};
use state::AppState;
use workspace::{
    current_workspace, open_workspace, recent_workspaces, workspace_diff, workspace_file,
    workspace_status, workspace_tree,
};

use night24_core::{
    provider::registry::ProviderRegistry, session::SessionManager, tool_executor::builtin_tools,
};
use night24_protocol::{
    PermissionDecision, PermissionMode, SkillRegistryParams, SubAgentPoolParams,
};

fn session_database_url() -> String {
    non_empty_env("NIGHT24_DATABASE_URL")
        .map(|value| normalize_sqlite_database_url(&value))
        .unwrap_or_else(default_session_database_url)
}

fn default_session_database_url() -> String {
    let path = data_dir().join("night24.db");
    sqlite_file_url(path)
}

fn data_dir() -> PathBuf {
    non_empty_env("NIGHT24_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(trimmed_non_empty)
}

fn trimmed_non_empty(value: impl AsRef<str>) -> Option<String> {
    let trimmed = value.as_ref().trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn trimmed_or_default(value: Option<&str>, default: &str) -> String {
    value
        .and_then(trimmed_non_empty)
        .unwrap_or_else(|| default.to_string())
}

fn env_or_default(key: &str, default: &str) -> String {
    trimmed_or_default(std::env::var(key).ok().as_deref(), default)
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
        reply::stream_run_events,
        sessions::list_sessions,
        sessions::get_session_history,
        sessions::create_session,
        sessions::compact_session,
        sessions::rename_session,
        sessions::fork_session,
        workspace::workspace_status,
        workspace::workspace_diff,
        hooks::get_workspace_hooks,
        hooks::put_workspace_hooks,
        get_agent_subagents,
        restart_agent_core,
        get_workspace_skills,
        get_workspace_skill
    ),
    components(schemas(
        ReplyRequest,
        CreateSessionRequest,
        CompactSessionRequest,
        CompactSessionResponse,
        RenameSessionRequest,
        ForkSessionRequest,
        SessionSummary,
        hooks::HookConfig,
        hooks::HookDefinition,
        hooks::HookConfigResponse
    )),
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
    let run_events = Arc::new(RunEventStore::new(data_dir().join("run-events")));
    let runner_mode = RunnerMode::from_env();
    info!(runner_mode = runner_mode.as_str(), "selected agent runner");
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
    let core_client = Arc::new(tokio::sync::RwLock::new(core_client));
    let agent_runner = build_agent_runner(runner_mode, core_client.clone());

    let app_state = AppState {
        session_manager,
        provider_registry: Arc::new(provider_registry),
        permission_manager: Arc::new(permission_manager),
        workspace_state: Arc::new(tokio::sync::RwLock::new(WorkspaceState::new())),
        core_client,
        agent_runner,
        runner_mode,
        run_events,
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/reply", post(reply_core))
        .route("/runs/{run_id}/events", get(stream_run_events))
        .route("/agent/cancel", post(agent_cancel))
        .route("/agent/core/restart", post(restart_agent_core))
        .route("/agent/subagents", get(get_agent_subagents))
        .route("/tools", get(get_tools))
        .route("/workspaces/open", post(open_workspace))
        .route("/workspaces/current", get(current_workspace))
        .route("/workspaces/recent", get(recent_workspaces))
        .route("/workspace/tree", get(workspace_tree))
        .route("/workspace/file", get(workspace_file))
        .route("/workspace/status", get(workspace_status))
        .route("/workspace/diff", get(workspace_diff))
        .route("/workspace/skills", get(get_workspace_skills))
        .route("/workspace/skills/{name}", get(get_workspace_skill))
        .route(
            "/workspace/hooks",
            get(get_workspace_hooks).put(put_workspace_hooks),
        )
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{id}", delete(delete_session))
        .route("/sessions/{id}/history", get(get_session_history))
        .route("/sessions/{id}/compact", post(compact_session))
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
    let app = if let Some(api_key) = non_empty_env("NIGHT24_API_KEY") {
        info!("API key authentication enabled");
        app.layer(axum::middleware::from_fn_with_state(
            api_key,
            require_api_key,
        ))
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

    if let Some(api_key) = non_empty_env("OPENAI_API_KEY") {
        let base_url = env_or_default("OPENAI_BASE_URL", "https://api.openai.com/v1");
        let model = env_or_default("OPENAI_MODEL", "gpt-4o-mini");
        registry = registry.with_openai(api_key, base_url, model);
    }

    if let Some(api_key) = non_empty_env("ANTHROPIC_API_KEY") {
        let base_url = env_or_default("ANTHROPIC_BASE_URL", "https://api.anthropic.com");
        let model = env_or_default("ANTHROPIC_MODEL", "claude-3-5-sonnet-latest");
        registry = registry.with_anthropic(api_key, base_url, model);
    }

    if let Some(api_key) = non_empty_env("STEPFUN_API_KEY") {
        let base_url = env_or_default("STEPFUN_BASE_URL", "https://api.stepfun.com/step_plan/v1");
        let model = env_or_default("STEPFUN_MODEL", "step-3.7-flash");
        registry = registry.with_stepfun(api_key, base_url, model);
    }

    let ollama_base_url = env_or_default("OLLAMA_BASE_URL", "http://localhost:11434");
    let ollama_model = env_or_default("OLLAMA_MODEL", "llama3.2");
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
    let mode_value = std::env::var("NIGHT24_PERMISSION_MODE").ok();
    let mode = PermissionMode::normalize(mode_value.as_deref());
    match mode {
        PermissionMode::Permissive => {
            info!(permission_mode = %mode.as_str(), "permission: permissive (read-only allowed)");
            PermissionManager::permissive_local()
        }
        PermissionMode::AllowAll => {
            info!(permission_mode = %mode.as_str(), "permission: allow_all (NOT for production)");
            PermissionManager::new(PermissionLevel::Allow)
        }
        PermissionMode::DenyAll => {
            info!(permission_mode = %mode.as_str(), "permission: deny_all");
            PermissionManager::new(PermissionLevel::Deny)
        }
        PermissionMode::Strict => {
            info!(
                permission_mode = %mode.as_str(),
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

async fn current_core_client(state: &AppState) -> Option<Arc<AgentCoreClient>> {
    state.core_client.read().await.clone()
}

fn current_agent_runner(state: &AppState) -> Arc<dyn AgentRunner> {
    state.agent_runner.clone()
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
    let core = match current_core_client(&state).await {
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
    if let Some(core_client) = current_core_client(&state).await {
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
    get,
    path = "/agent/subagents",
    tag = "night24",
    params(SubAgentPoolQuery),
    responses(
        (status = 200, description = "Current sub-agent pool status", body = serde_json::Value)
    )
)]
async fn get_agent_subagents(
    State(state): State<AppState>,
    Query(query): Query<SubAgentPoolQuery>,
) -> Json<serde_json::Value> {
    let Some(core_client) = current_core_client(&state).await else {
        return Json(serde_json::json!({
            "core_available": false,
            "error": "no active core client"
        }));
    };

    match core_client
        .subagents(SubAgentPoolParams {
            subagent_id: query.subagent_id,
            include_messages: query.include_messages,
            include_result: query.include_result,
        })
        .await
    {
        Ok(result) => Json(serde_json::json!({
            "core_available": true,
            "pool": result.pool
        })),
        Err(err) => Json(serde_json::json!({
            "core_available": false,
            "error": err.to_string()
        })),
    }
}

#[utoipa::path(
    post,
    path = "/agent/core/restart",
    tag = "night24",
    responses(
        (status = 200, description = "Agent Core restart status", body = CoreRestartResponse)
    )
)]
async fn restart_agent_core(State(state): State<AppState>) -> Json<CoreRestartResponse> {
    let result = match current_core_client(&state).await {
        Some(core_client) => core_client.restart().await.map(|_| core_client),
        None => AgentCoreClient::spawn().await.map(Arc::new),
    };

    match result {
        Ok(core_client) => {
            {
                let mut guard = state.core_client.write().await;
                *guard = Some(core_client.clone());
            }
            let core = core_client.status().await;
            Json(CoreRestartResponse {
                accepted: true,
                reason: None,
                core: CoreStatusResponse {
                    available: core.available,
                    initialized: core.initialized,
                    reason: core.reason,
                },
            })
        }
        Err(err) => {
            {
                let mut guard = state.core_client.write().await;
                *guard = None;
            }
            Json(CoreRestartResponse {
                accepted: false,
                reason: Some(err.to_string()),
                core: CoreStatusResponse {
                    available: false,
                    initialized: false,
                    reason: Some(err.to_string()),
                },
            })
        }
    }
}

#[utoipa::path(
    get,
    path = "/workspace/skills",
    tag = "night24",
    responses(
        (status = 200, description = "Current workspace skill registry", body = serde_json::Value),
        (status = 409, description = "No workspace is open")
    )
)]
async fn get_workspace_skills(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let workspace = workspace::current_workspace_info(&state).await?;
    let Some(core_client) = current_core_client(&state).await else {
        return Err(json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "no active core client",
        ));
    };
    let result = core_client
        .skills(SkillRegistryParams {
            working_dir: Some(PathBuf::from(&workspace.root_path)),
        })
        .await
        .map_err(|err| json_error(StatusCode::BAD_GATEWAY, err.to_string()))?;
    Ok(Json(serde_json::json!({
        "workspace": workspace,
        "registry": result.registry
    })))
}

#[utoipa::path(
    get,
    path = "/workspace/skills/{name}",
    tag = "night24",
    params(
        ("name" = String, Path, description = "Skill name"),
        SkillLoadQuery
    ),
    responses(
        (status = 200, description = "Loaded workspace skill details", body = serde_json::Value),
        (status = 409, description = "No workspace is open")
    )
)]
async fn get_workspace_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(query): Query<SkillLoadQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let workspace = workspace::current_workspace_info(&state).await?;
    let Some(core_client) = current_core_client(&state).await else {
        return Err(json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "no active core client",
        ));
    };
    let result = core_client
        .load_skill(night24_protocol::SkillLoadParams {
            working_dir: Some(PathBuf::from(&workspace.root_path)),
            name,
            file: query.file,
        })
        .await
        .map_err(|err| json_error(StatusCode::BAD_GATEWAY, err.to_string()))?;
    Ok(Json(serde_json::json!({
        "workspace": workspace,
        "skill": result.skill
    })))
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
    if let Some(run_id) = req.run_id.clone() {
        let agent_runner = current_agent_runner(&state);
        match agent_runner
            .cancel(run_id.clone(), req.reason.clone())
            .await
        {
            Ok(_) => {
                return accepted_response(true, None, Some(run_id), None);
            }
            Err(err) => {
                return accepted_response(false, Some(err.to_string()), Some(run_id), None);
            }
        }
    }
    let reason = req
        .reason
        .map(|reason| format!("no active core client or run_id: {}", reason))
        .unwrap_or_else(|| "no active core client or run_id".to_string());
    accepted_response(false, Some(reason), req.run_id, None)
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
    handle_permission_decision(state, id, req, PermissionDecision::Approve).await
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
    handle_permission_decision(state, id, req, PermissionDecision::Deny).await
}

async fn handle_permission_decision(
    state: AppState,
    id: String,
    req: PermissionDecisionRequest,
    decision: PermissionDecision,
) -> Json<AcceptedResponse> {
    if let Some(run_id) = req.run_id.clone() {
        let agent_runner = current_agent_runner(&state);
        match agent_runner
            .resolve_permission(run_id.clone(), id.clone(), decision, req.reason.clone())
            .await
        {
            Ok(_) => {
                return accepted_response(true, None, Some(run_id), Some(id));
            }
            Err(err) => {
                return accepted_response(false, Some(err.to_string()), Some(run_id), Some(id));
            }
        }
    }
    let detail = req
        .reason
        .unwrap_or_else(|| "no active core client or run_id".to_string());
    accepted_response(false, Some(detail), req.run_id, Some(id))
}

fn accepted_response(
    accepted: bool,
    reason: Option<String>,
    run_id: Option<String>,
    permission_id: Option<String>,
) -> Json<AcceptedResponse> {
    Json(AcceptedResponse {
        accepted,
        reason,
        run_id,
        permission_id,
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

    #[test]
    fn test_trimmed_non_empty_ignores_blank_values() {
        assert_eq!(
            trimmed_non_empty("  data/night24.db  ").as_deref(),
            Some("data/night24.db")
        );
        assert_eq!(trimmed_non_empty(" \t\r\n "), None);
    }

    #[test]
    fn test_trimmed_or_default_trims_and_falls_back() {
        assert_eq!(
            trimmed_or_default(Some(" https://api.example.com/v1 "), "default"),
            "https://api.example.com/v1"
        );
        assert_eq!(trimmed_or_default(Some(" \t\r\n "), "default"), "default");
        assert_eq!(trimmed_or_default(None, "default"), "default");
    }

    #[test]
    fn test_accepted_response_preserves_optional_ids() {
        let Json(response) = accepted_response(
            false,
            Some("missing core".to_string()),
            Some("run-1".to_string()),
            Some("permission-1".to_string()),
        );

        assert!(!response.accepted);
        assert_eq!(response.reason.as_deref(), Some("missing core"));
        assert_eq!(response.run_id.as_deref(), Some("run-1"));
        assert_eq!(response.permission_id.as_deref(), Some("permission-1"));
    }
}
