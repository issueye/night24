use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use axum::{
    body::Body,
    extract::{Path, Query, Request, State},
    http::{header, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::delete,
    routing::get,
    routing::post,
    routing::put,
    Json, Router,
};
use futures::stream;
use serde::{Deserialize, Serialize};
use tokio::signal;
use tokio::sync::{mpsc, oneshot};
use tower_http::cors::CorsLayer;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use night24_core::{
    agent::{Agent, AgentConfig},
    model::{ContentBlock, Message, Role},
    provider::{registry::ProviderRegistry, ModelConfig},
    session::{Session, SessionManager, SessionType},
    tool_executor::builtin_tools,
};
use night24_protocol::{
    AgentToolsResult, CancelParams, Capability, InitializeEnvironment, InitializeParams,
    JsonRpcRequest, PeerInfo, PermissionDecision, PermissionResolution, ProviderConfig,
    ReplyAccepted, ReplyInput, ReplyLimits, ReplyOptions, ReplyParams, ReplySession,
    PROTOCOL_VERSION,
};

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[allow(dead_code)]
struct ReplyRequest {
    #[schema(example = "hello world")]
    #[serde(alias = "message")]
    text: String,
    #[schema(example = "echo")]
    provider: Option<String>,
    #[schema(example = "sk-...")]
    api_key: Option<String>,
    #[schema(example = "https://api.openai.com/v1")]
    base_url: Option<String>,
    #[schema(example = "gpt-4o-mini")]
    model: Option<String>,
    #[schema(example = "session-123")]
    session_id: Option<String>,
    #[schema(example = "allow_all")]
    permission_mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
struct OpenWorkspaceRequest {
    #[schema(example = "E:\\code\\issueye\\ai_agent\\night24")]
    path: String,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
struct WorkspacePathQuery {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
struct WorkspaceDiffQuery {
    #[serde(default)]
    staged: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
struct CancelAgentRequest {
    #[schema(example = "run-123")]
    run_id: Option<String>,
    #[schema(example = "user_cancelled")]
    reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
struct PermissionDecisionRequest {
    #[schema(example = "run-123")]
    run_id: Option<String>,
    #[schema(example = "user denied running this command")]
    reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
struct CreateSessionRequest {
    #[schema(example = "my-chat")]
    name: Option<String>,
    #[schema(example = "user")]
    session_type: Option<String>,
    #[schema(example = "E:\\code\\project")]
    working_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
struct RenameSessionRequest {
    #[schema(example = "debugging rust")]
    name: String,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
struct ForkSessionRequest {
    /// Optional index at which to fork. If omitted, the full history is copied.
    #[schema(example = 4)]
    at_index: Option<usize>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
struct SessionSummary {
    id: String,
    name: String,
    session_type: String,
    working_dir: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
struct ReadyResponse {
    ready: bool,
    server: String,
    core: CoreStatusResponse,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
struct CoreStatusResponse {
    available: bool,
    initialized: bool,
    reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
struct WorkspaceInfo {
    id: String,
    name: String,
    root_path: String,
    created_at: String,
    last_opened_at: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
struct RecentWorkspacesResponse {
    workspaces: Vec<WorkspaceInfo>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
struct WorkspaceTreeResponse {
    workspace: WorkspaceInfo,
    root: FileTreeNode,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
struct FileTreeNode {
    name: String,
    path: String,
    kind: String,
    children: Option<Vec<FileTreeNode>>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
struct WorkspaceFileResponse {
    path: String,
    name: String,
    size: u64,
    is_binary: bool,
    content: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
struct WorkspaceStatusResponse {
    workspace: WorkspaceInfo,
    is_git_repo: bool,
    branch: Option<String>,
    files: Vec<WorkspaceStatusFile>,
    has_changes: bool,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
struct WorkspaceStatusFile {
    path: String,
    index_status: String,
    worktree_status: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
struct WorkspaceDiffResponse {
    workspace: WorkspaceInfo,
    staged: bool,
    diff: String,
    files_changed: usize,
    insertions: usize,
    deletions: usize,
    has_changes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceChangeSnapshot {
    status: String,
    diff: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
struct ToolsResponse {
    tools: Vec<night24_core::model::Tool>,
    source: String,
    core_available: bool,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
struct AcceptedResponse {
    accepted: bool,
    reason: Option<String>,
    run_id: Option<String>,
    permission_id: Option<String>,
}

#[derive(Debug, Clone)]
struct WorkspaceState {
    current: Option<WorkspaceInfo>,
    recent: Vec<WorkspaceInfo>,
}

impl WorkspaceState {
    fn new() -> Self {
        Self {
            current: None,
            recent: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct CoreRuntimeStatus {
    available: bool,
    initialized: bool,
    reason: Option<String>,
}

impl CoreRuntimeStatus {
    fn available() -> Self {
        Self {
            available: true,
            initialized: true,
            reason: None,
        }
    }

    fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            initialized: false,
            reason: Some(reason.into()),
        }
    }
}

struct AgentCoreClient {
    stdin: Arc<Mutex<ChildStdin>>,
    child: Arc<Mutex<Child>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>>,
    event_senders: Arc<Mutex<HashMap<String, mpsc::Sender<serde_json::Value>>>>,
    status: Arc<Mutex<CoreRuntimeStatus>>,
}

impl AgentCoreClient {
    async fn spawn() -> anyhow::Result<Self> {
        let bin = locate_agent_core_bin();
        let mut child = Command::new(&bin)
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| anyhow::anyhow!("failed to spawn {}: {}", bin.display(), err))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("agent-core stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("agent-core stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("agent-core stderr unavailable"))?;

        let client = Self {
            stdin: Arc::new(Mutex::new(stdin)),
            child: Arc::new(Mutex::new(child)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            event_senders: Arc::new(Mutex::new(HashMap::new())),
            status: Arc::new(Mutex::new(CoreRuntimeStatus::unavailable("initializing"))),
        };

        client.start_stdout_reader(stdout);
        client.start_stderr_reader(stderr);
        client.initialize().await?;
        if let Ok(mut status) = client.status.lock() {
            *status = CoreRuntimeStatus::available();
        }
        Ok(client)
    }

    async fn status(&self) -> CoreRuntimeStatus {
        if let Ok(mut child) = self.child.lock() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let status = CoreRuntimeStatus::unavailable(format!(
                        "agent-core exited with status {}",
                        status
                    ));
                    if let Ok(mut guard) = self.status.lock() {
                        *guard = status.clone();
                    }
                    return status;
                }
                Ok(None) => {}
                Err(err) => {
                    let status = CoreRuntimeStatus::unavailable(format!(
                        "agent-core status check failed: {err}"
                    ));
                    if let Ok(mut guard) = self.status.lock() {
                        *guard = status.clone();
                    }
                    return status;
                }
            }
        }
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_else(|_| CoreRuntimeStatus::unavailable("agent-core status lock poisoned"))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        let params = InitializeParams {
            protocol_version: PROTOCOL_VERSION.to_string(),
            client: PeerInfo::new("night24-server", env!("CARGO_PKG_VERSION")),
            workspace_root: None,
            environment: InitializeEnvironment {
                permission_mode: std::env::var("NIGHT24_PERMISSION_MODE").ok(),
                default_provider: Some("echo".to_string()),
            },
            capabilities: vec![
                Capability::new("agent.cancel", 1),
                Capability::new("permission.resolve", 1),
            ],
        };
        self.call("core.initialize", params, Duration::from_secs(5))
            .await
            .map(|_| ())
    }

    async fn call(
        &self,
        method: &str,
        params: impl serde::Serialize,
        timeout: Duration,
    ) -> anyhow::Result<serde_json::Value> {
        let id = format!("rpc-{}", uuid::Uuid::new_v4());
        let request = JsonRpcRequest::new(id.clone(), method, params)?;
        let line = serde_json::to_string(&request)?;
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|_| anyhow::anyhow!("agent-core pending lock poisoned"))?
            .insert(id.clone(), tx);

        let write_result = {
            let mut stdin = self
                .stdin
                .lock()
                .map_err(|_| anyhow::anyhow!("agent-core stdin lock poisoned"))?;
            writeln!(stdin, "{line}").and_then(|_| stdin.flush())
        };

        if let Err(err) = write_result {
            self.pending
                .lock()
                .map_err(|_| anyhow::anyhow!("agent-core pending lock poisoned"))?
                .remove(&id);
            if let Ok(mut status) = self.status.lock() {
                *status = CoreRuntimeStatus::unavailable(format!("agent-core write failed: {err}"));
            }
            return Err(anyhow::anyhow!("agent-core write failed: {err}"));
        }

        let response = tokio::time::timeout(timeout, rx)
            .await
            .map_err(|_| anyhow::anyhow!("agent-core request timed out: {method}"))?
            .map_err(|_| anyhow::anyhow!("agent-core response channel closed"))?;

        if let Some(error) = response.get("error") {
            return Err(anyhow::anyhow!("agent-core {method} failed: {error}"));
        }

        Ok(response
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    async fn tools(&self) -> anyhow::Result<AgentToolsResult> {
        let result = self
            .call(
                "agent.tools",
                serde_json::json!({ "include_disabled": false }),
                Duration::from_secs(5),
            )
            .await?;
        serde_json::from_value(result).map_err(Into::into)
    }

    async fn reply(
        &self,
        params: ReplyParams,
    ) -> anyhow::Result<(ReplyAccepted, mpsc::Receiver<serde_json::Value>)> {
        let run_id = params.run_id.clone();
        let (tx, rx) = mpsc::channel(64);
        self.event_senders
            .lock()
            .map_err(|_| anyhow::anyhow!("agent-core events lock poisoned"))?
            .insert(run_id.clone(), tx);

        match self
            .call("agent.reply", params, Duration::from_secs(10))
            .await
        {
            Ok(result) => {
                let accepted = serde_json::from_value(result)?;
                Ok((accepted, rx))
            }
            Err(err) => {
                self.event_senders
                    .lock()
                    .map_err(|_| anyhow::anyhow!("agent-core events lock poisoned"))?
                    .remove(&run_id);
                Err(err)
            }
        }
    }

    async fn cancel(
        &self,
        run_id: String,
        reason: Option<String>,
    ) -> anyhow::Result<serde_json::Value> {
        self.call(
            "agent.cancel",
            CancelParams { run_id, reason },
            Duration::from_secs(5),
        )
        .await
    }

    async fn resolve_permission(
        &self,
        run_id: String,
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
    ) -> anyhow::Result<serde_json::Value> {
        self.call(
            "permission.resolve",
            PermissionResolution {
                run_id,
                permission_id,
                decision,
                reason,
            },
            Duration::from_secs(5),
        )
        .await
    }

    fn start_stdout_reader(&self, stdout: std::process::ChildStdout) {
        let pending = self.pending.clone();
        let event_senders = self.event_senders.clone();
        let status = self.status.clone();
        thread::spawn(move || {
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines() {
                let line = match line {
                    Ok(line) => line,
                    Err(err) => {
                        set_core_status(
                            &status,
                            CoreRuntimeStatus::unavailable(format!(
                                "agent-core stdout read failed: {err}"
                            )),
                        );
                        break;
                    }
                };
                if line.trim().is_empty() {
                    continue;
                }

                let value: serde_json::Value = match serde_json::from_str(&line) {
                    Ok(value) => value,
                    Err(err) => {
                        set_core_status(
                            &status,
                            CoreRuntimeStatus::unavailable(format!(
                                "agent-core stdout protocol violation: {err}"
                            )),
                        );
                        continue;
                    }
                };

                if value.get("method").and_then(|method| method.as_str()) == Some("agent.event") {
                    if let Some(params) = value.get("params").cloned() {
                        let run_id = params
                            .get("run_id")
                            .and_then(|run_id| run_id.as_str())
                            .map(|run_id| run_id.to_string());
                        let is_terminal = params
                            .get("type")
                            .and_then(|kind| kind.as_str())
                            .map(|kind| kind == "finish" || kind == "error")
                            .unwrap_or(false);
                        if let Some(run_id) = run_id {
                            let sender = event_senders
                                .lock()
                                .ok()
                                .and_then(|guard| guard.get(&run_id).cloned());
                            if let Some(sender) = sender {
                                let _ = sender.blocking_send(params);
                            }
                            if is_terminal {
                                if let Ok(mut guard) = event_senders.lock() {
                                    guard.remove(&run_id);
                                }
                            }
                        }
                    }
                    continue;
                }

                if let Some(id) = value.get("id").and_then(json_rpc_id_key) {
                    let tx = pending.lock().ok().and_then(|mut guard| guard.remove(&id));
                    if let Some(tx) = tx {
                        let _ = tx.send(value);
                    }
                }
            }
        });
    }

    fn start_stderr_reader(&self, stderr: std::process::ChildStderr) {
        thread::spawn(move || {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                warn!(target: "night24_agent_core", "{}", line);
            }
        });
    }
}

fn set_core_status(status_ptr: &Arc<Mutex<CoreRuntimeStatus>>, status: CoreRuntimeStatus) {
    if let Ok(mut guard) = status_ptr.lock() {
        *guard = status.clone();
    }
    warn!(reason = ?status.reason, "agent-core became unavailable");
}

fn json_rpc_id_key(value: &serde_json::Value) -> Option<String> {
    if let Some(value) = value.as_str() {
        return Some(value.to_string());
    }
    value.as_i64().map(|value| value.to_string())
}

fn locate_agent_core_bin() -> PathBuf {
    if let Ok(path) = std::env::var("NIGHT24_AGENT_CORE_BIN") {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }

    let exe_name = if cfg!(windows) {
        "night24-agent-core.exe"
    } else {
        "night24-agent-core"
    };

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let sibling = dir.join(exe_name);
            if sibling.exists() {
                return sibling;
            }
            if let Some(profile_root) = dir.parent() {
                for profile in ["release", "debug"] {
                    let candidate = profile_root.join(profile).join(exe_name);
                    if candidate.exists() {
                        return candidate;
                    }
                }
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        for profile in ["release", "debug"] {
            let candidate = cwd.join("target").join(profile).join(exe_name);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    PathBuf::from(exe_name)
}

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
        reply,
        list_sessions,
        get_session_history,
        create_session,
        rename_session,
        fork_session,
        workspace_status,
        workspace_diff
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

#[derive(Clone)]
#[allow(dead_code)]
struct AppState {
    session_manager: Arc<SessionManager>,
    provider_registry: Arc<ProviderRegistry>,
    permission_manager: Arc<night24_core::permission::PermissionManager>,
    workspace_state: Arc<tokio::sync::RwLock<WorkspaceState>>,
    core_client: Option<Arc<AgentCoreClient>>,
}

/// Whether a request path is exempt from authentication (health/docs).
fn is_public_path(path: &str) -> bool {
    path == "/healthz"
        || path == "/readyz"
        || path.starts_with("/swagger-ui")
        || path.starts_with("/api-docs")
}

/// Middleware that enforces an API key when `NIGHT24_API_KEY` is configured.
///
/// Accepted formats:
/// - `Authorization: Bearer <key>`
/// - `X-API-Key: <key>`
///
/// Public, unauthenticated paths: `/healthz`, `/swagger-ui*`, `/api-docs*`.
async fn require_api_key(
    State(expected_key): State<String>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if is_public_path(path) {
        return next.run(request).await;
    }

    let headers = request.headers();
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim().to_string())
        });

    match provided {
        Some(key) if constant_time_eq(key.as_bytes(), expected_key.as_bytes()) => {
            next.run(request).await
        }
        _ => (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "missing or invalid api key"})),
        )
            .into_response(),
    }
}

/// Compare two byte slices in constant time to avoid timing side channels.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_TREE_DEPTH: usize = 3;
const MAX_TREE_CHILDREN: usize = 200;

fn json_error(
    status: StatusCode,
    message: impl Into<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": message.into() })))
}

fn workspace_recents_limit() -> usize {
    std::env::var("NIGHT24_WORKSPACE_RECENTS_LIMIT")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|limit| *limit > 0)
        .unwrap_or(10)
}

fn canonical_workspace_root(path: &str) -> Result<PathBuf, (StatusCode, Json<serde_json::Value>)> {
    let raw = PathBuf::from(path);
    let canonical = raw
        .canonicalize()
        .map_err(|_| json_error(StatusCode::BAD_REQUEST, "workspace path does not exist"))?;
    if !canonical.is_dir() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "workspace path is not a directory",
        ));
    }
    Ok(canonical)
}

async fn current_workspace_info(
    state: &AppState,
) -> Result<WorkspaceInfo, (StatusCode, Json<serde_json::Value>)> {
    state
        .workspace_state
        .read()
        .await
        .current
        .clone()
        .ok_or_else(|| json_error(StatusCode::CONFLICT, "no workspace is open"))
}

async fn current_workspace_path(state: &AppState) -> Option<PathBuf> {
    state
        .workspace_state
        .read()
        .await
        .current
        .as_ref()
        .map(|workspace| PathBuf::from(&workspace.root_path))
}

fn resolve_workspace_existing_path(
    root: &FsPath,
    relative: Option<&str>,
) -> Result<PathBuf, (StatusCode, Json<serde_json::Value>)> {
    let canonical_root = root.canonicalize().map_err(|_| {
        json_error(
            StatusCode::CONFLICT,
            "current workspace root is unavailable",
        )
    })?;
    let relative = relative.unwrap_or("").trim();
    let relative_path = if relative.is_empty() || relative == "." {
        PathBuf::new()
    } else {
        let path = PathBuf::from(relative);
        if path.is_absolute() {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "workspace paths must be relative",
            ));
        }
        path
    };

    let candidate = canonical_root.join(relative_path);
    let canonical_candidate = candidate
        .canonicalize()
        .map_err(|_| json_error(StatusCode::NOT_FOUND, "workspace path not found"))?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "path escapes workspace root",
        ));
    }
    Ok(canonical_candidate)
}

fn workspace_relative_path(root: &FsPath, path: &FsPath) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let value = relative.to_string_lossy().replace('\\', "/");
    if value.is_empty() {
        ".".to_string()
    } else {
        value
    }
}

fn build_file_tree(
    root: &FsPath,
    path: &FsPath,
    depth: usize,
) -> Result<FileTreeNode, (StatusCode, Json<serde_json::Value>)> {
    let metadata = std::fs::metadata(path).map_err(|_| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to read workspace tree",
        )
    })?;
    let kind = if metadata.is_dir() {
        "directory"
    } else {
        "file"
    }
    .to_string();
    let name = if path == root {
        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("workspace")
            .to_string()
    } else {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    };

    let children = if metadata.is_dir() && depth < MAX_TREE_DEPTH {
        let mut entries = std::fs::read_dir(path)
            .map_err(|_| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to read workspace tree",
                )
            })?
            .filter_map(|entry| entry.ok())
            .filter(|entry| !should_skip_workspace_entry(&entry.path()))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| {
            let path = entry.path();
            let is_file = path.is_file();
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            (is_file, name)
        });

        let nodes = entries
            .into_iter()
            .take(MAX_TREE_CHILDREN)
            .filter_map(|entry| build_file_tree(root, &entry.path(), depth + 1).ok())
            .collect::<Vec<_>>();
        Some(nodes)
    } else if metadata.is_dir() {
        Some(Vec::new())
    } else {
        None
    };

    Ok(FileTreeNode {
        name,
        path: workspace_relative_path(root, path),
        kind,
        children,
    })
}

fn should_skip_workspace_entry(path: &FsPath) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            matches!(
                name,
                ".git" | "target" | "node_modules" | ".venv" | "venv" | "__pycache__"
            )
        })
        .unwrap_or(false)
}

fn is_binary_bytes(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|byte| *byte == 0)
}

fn run_git(root: &FsPath, args: &[&str]) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to run git"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            "git command failed".to_string()
        } else {
            stderr
        };
        return Err(json_error(StatusCode::BAD_REQUEST, message));
    }

    String::from_utf8(output.stdout)
        .map_err(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "git output is not utf-8"))
}

fn ensure_git_workspace(root: &FsPath) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let inside = run_git(root, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.trim() == "true" {
        Ok(())
    } else {
        Err(json_error(
            StatusCode::BAD_REQUEST,
            "workspace is not a git repository",
        ))
    }
}

fn parse_git_status(raw: &str) -> Vec<WorkspaceStatusFile> {
    raw.lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let index_status = line.chars().next().unwrap_or(' ').to_string();
            let worktree_status = line.chars().nth(1).unwrap_or(' ').to_string();
            let path = line[3..].split(" -> ").last().unwrap_or("").to_string();
            if path.is_empty() {
                None
            } else {
                Some(WorkspaceStatusFile {
                    path,
                    index_status,
                    worktree_status,
                })
            }
        })
        .collect()
}

fn diff_stats(diff: &str) -> (usize, usize, usize) {
    let mut files_changed = 0;
    let mut insertions = 0;
    let mut deletions = 0;

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            files_changed += 1;
        } else if line.starts_with('+') && !line.starts_with("+++") {
            insertions += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            deletions += 1;
        }
    }

    (files_changed, insertions, deletions)
}

fn workspace_change_snapshot(
    root: &FsPath,
) -> Result<WorkspaceChangeSnapshot, (StatusCode, Json<serde_json::Value>)> {
    ensure_git_workspace(root)?;
    Ok(WorkspaceChangeSnapshot {
        status: run_git(root, &["status", "--porcelain=v1"])?,
        diff: run_git(root, &["diff", "--no-ext-diff"])?,
    })
}

fn build_diff_ready_event(
    run_id: &str,
    seq: u64,
    root: &FsPath,
    baseline: Option<&WorkspaceChangeSnapshot>,
) -> Option<serde_json::Value> {
    let current = workspace_change_snapshot(root).ok()?;
    if baseline.is_some_and(|baseline| baseline == &current) {
        return None;
    }
    if current.status.trim().is_empty() && current.diff.trim().is_empty() {
        return None;
    }

    let (diff_files, insertions, deletions) = diff_stats(&current.diff);
    let status_files = parse_git_status(&current.status).len();
    let files_changed = diff_files.max(status_files);
    Some(serde_json::json!({
        "type": "diff_ready",
        "run_id": run_id,
        "seq": seq,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "payload": {
            "files_changed": files_changed,
            "insertions": insertions,
            "deletions": deletions,
            "summary": format!("{} files changed (+{} / -{})", files_changed, insertions, deletions)
        }
    }))
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

#[utoipa::path(
    post,
    path = "/workspaces/open",
    tag = "night24",
    request_body = OpenWorkspaceRequest,
    responses(
        (status = 200, description = "Opened workspace", body = WorkspaceInfo),
        (status = 400, description = "Invalid workspace path")
    )
)]
async fn open_workspace(
    State(state): State<AppState>,
    Json(req): Json<OpenWorkspaceRequest>,
) -> Result<Json<WorkspaceInfo>, (StatusCode, Json<serde_json::Value>)> {
    let root = canonical_workspace_root(&req.path)?;
    let now = chrono::Utc::now().to_rfc3339();
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("workspace")
        .to_string();
    let root_path = root.to_string_lossy().to_string();
    let workspace = WorkspaceInfo {
        id: format!("workspace-{}", uuid::Uuid::new_v4()),
        name,
        root_path: root_path.clone(),
        created_at: now.clone(),
        last_opened_at: now,
    };

    let mut guard = state.workspace_state.write().await;
    guard.recent.retain(|item| item.root_path != root_path);
    guard.recent.insert(0, workspace.clone());
    let limit = workspace_recents_limit();
    guard.recent.truncate(limit);
    guard.current = Some(workspace.clone());

    Ok(Json(workspace))
}

#[utoipa::path(
    get,
    path = "/workspaces/current",
    tag = "night24",
    responses(
        (status = 200, description = "Current workspace", body = Option<WorkspaceInfo>)
    )
)]
async fn current_workspace(State(state): State<AppState>) -> Json<Option<WorkspaceInfo>> {
    Json(state.workspace_state.read().await.current.clone())
}

#[utoipa::path(
    get,
    path = "/workspaces/recent",
    tag = "night24",
    responses(
        (status = 200, description = "Recent workspaces", body = RecentWorkspacesResponse)
    )
)]
async fn recent_workspaces(State(state): State<AppState>) -> Json<RecentWorkspacesResponse> {
    Json(RecentWorkspacesResponse {
        workspaces: state.workspace_state.read().await.recent.clone(),
    })
}

#[utoipa::path(
    get,
    path = "/workspace/tree",
    tag = "night24",
    params(
        ("path" = Option<String>, Query, description = "Relative path under the current workspace")
    ),
    responses(
        (status = 200, description = "Workspace file tree", body = WorkspaceTreeResponse),
        (status = 409, description = "No workspace is open")
    )
)]
async fn workspace_tree(
    State(state): State<AppState>,
    Query(query): Query<WorkspacePathQuery>,
) -> Result<Json<WorkspaceTreeResponse>, (StatusCode, Json<serde_json::Value>)> {
    let workspace = current_workspace_info(&state).await?;
    let root = PathBuf::from(&workspace.root_path);
    let target = resolve_workspace_existing_path(&root, query.path.as_deref())?;
    if !target.is_dir() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "path is not a directory",
        ));
    }

    let node = build_file_tree(&root, &target, 0)?;
    Ok(Json(WorkspaceTreeResponse {
        workspace,
        root: node,
    }))
}

#[utoipa::path(
    get,
    path = "/workspace/file",
    tag = "night24",
    params(
        ("path" = String, Query, description = "Relative file path under the current workspace")
    ),
    responses(
        (status = 200, description = "Workspace file content", body = WorkspaceFileResponse),
        (status = 409, description = "No workspace is open")
    )
)]
async fn workspace_file(
    State(state): State<AppState>,
    Query(query): Query<WorkspacePathQuery>,
) -> Result<Json<WorkspaceFileResponse>, (StatusCode, Json<serde_json::Value>)> {
    let workspace = current_workspace_info(&state).await?;
    let root = PathBuf::from(&workspace.root_path);
    let relative = query
        .path
        .as_deref()
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| json_error(StatusCode::BAD_REQUEST, "missing file path"))?;
    let target = resolve_workspace_existing_path(&root, Some(relative))?;
    if !target.is_file() {
        return Err(json_error(StatusCode::BAD_REQUEST, "path is not a file"));
    }

    let metadata = std::fs::metadata(&target).map_err(|_| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to read file metadata",
        )
    })?;
    let relative_path = workspace_relative_path(&root, &target);
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    if metadata.len() > MAX_FILE_BYTES {
        return Ok(Json(WorkspaceFileResponse {
            path: relative_path,
            name,
            size: metadata.len(),
            is_binary: false,
            content: None,
            error: Some(format!("file is larger than {} bytes", MAX_FILE_BYTES)),
        }));
    }

    let bytes = std::fs::read(&target)
        .map_err(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to read file"))?;
    if is_binary_bytes(&bytes) {
        return Ok(Json(WorkspaceFileResponse {
            path: relative_path,
            name,
            size: metadata.len(),
            is_binary: true,
            content: None,
            error: Some("binary files cannot be previewed".to_string()),
        }));
    }

    match String::from_utf8(bytes) {
        Ok(content) => Ok(Json(WorkspaceFileResponse {
            path: relative_path,
            name,
            size: metadata.len(),
            is_binary: false,
            content: Some(content),
            error: None,
        })),
        Err(_) => Ok(Json(WorkspaceFileResponse {
            path: relative_path,
            name,
            size: metadata.len(),
            is_binary: true,
            content: None,
            error: Some("file is not valid UTF-8 text".to_string()),
        })),
    }
}

#[utoipa::path(
    get,
    path = "/workspace/status",
    tag = "night24",
    responses(
        (status = 200, description = "Workspace git status", body = WorkspaceStatusResponse),
        (status = 409, description = "No workspace is open")
    )
)]
async fn workspace_status(
    State(state): State<AppState>,
) -> Result<Json<WorkspaceStatusResponse>, (StatusCode, Json<serde_json::Value>)> {
    let workspace = current_workspace_info(&state).await?;
    let root = PathBuf::from(&workspace.root_path);
    ensure_git_workspace(&root)?;

    let branch = run_git(&root, &["branch", "--show-current"])
        .ok()
        .map(|branch| branch.trim().to_string())
        .filter(|branch| !branch.is_empty());
    let raw = run_git(&root, &["status", "--porcelain=v1"])?;
    let files = parse_git_status(&raw);
    Ok(Json(WorkspaceStatusResponse {
        workspace,
        is_git_repo: true,
        branch,
        has_changes: !files.is_empty(),
        files,
    }))
}

#[utoipa::path(
    get,
    path = "/workspace/diff",
    tag = "night24",
    params(
        ("staged" = Option<bool>, Query, description = "Return staged diff instead of worktree diff")
    ),
    responses(
        (status = 200, description = "Workspace git diff", body = WorkspaceDiffResponse),
        (status = 409, description = "No workspace is open")
    )
)]
async fn workspace_diff(
    State(state): State<AppState>,
    Query(query): Query<WorkspaceDiffQuery>,
) -> Result<Json<WorkspaceDiffResponse>, (StatusCode, Json<serde_json::Value>)> {
    let workspace = current_workspace_info(&state).await?;
    let root = PathBuf::from(&workspace.root_path);
    ensure_git_workspace(&root)?;

    let staged = query.staged.unwrap_or(false);
    let diff = if staged {
        run_git(&root, &["diff", "--cached", "--no-ext-diff"])?
    } else {
        run_git(&root, &["diff", "--no-ext-diff"])?
    };
    let (files_changed, insertions, deletions) = diff_stats(&diff);

    Ok(Json(WorkspaceDiffResponse {
        workspace,
        staged,
        has_changes: !diff.trim().is_empty(),
        diff,
        files_changed,
        insertions,
        deletions,
    }))
}

#[utoipa::path(
    get,
    path = "/sessions",
    tag = "night24",
    responses(
        (status = 200, description = "List all sessions", body = Vec<SessionSummary>)
    )
)]
async fn list_sessions(
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
async fn delete_session(
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
async fn create_session(
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
    let session_type = match req.session_type.as_deref() {
        Some("scheduled") => SessionType::Scheduled,
        Some("sub_agent") => SessionType::SubAgent,
        Some("hidden") => SessionType::Hidden,
        Some("terminal") => SessionType::Terminal,
        Some("gateway") => SessionType::Gateway,
        Some("acp") => SessionType::Acp,
        _ => SessionType::User,
    };
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
async fn get_session_history(
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
async fn rename_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<RenameSessionRequest>,
) -> Result<Json<Session>, (axum::http::StatusCode, String)> {
    match state.session_manager.rename(&id, req.name).await {
        Ok(session) => Ok(Json(session)),
        Err(e) => {
            if e.to_string().contains("not found") {
                Err((axum::http::StatusCode::NOT_FOUND, e.to_string()))
            } else {
                Err((axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
            }
        }
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
async fn fork_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ForkSessionRequest>,
) -> Result<Json<Session>, (axum::http::StatusCode, String)> {
    match state.session_manager.fork(&id, req.at_index).await {
        Ok(session) => Ok(Json(session)),
        Err(e) => {
            if e.to_string().contains("not found") {
                Err((axum::http::StatusCode::NOT_FOUND, e.to_string()))
            } else {
                Err((axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
            }
        }
    }
}

async fn reply_core(State(state): State<AppState>, Json(req): Json<ReplyRequest>) -> Response {
    let core_client = match &state.core_client {
        Some(core_client) => core_client.clone(),
        None => {
            return sse_error_response(None, "core_unavailable", "no active core client", true);
        }
    };

    let session = if let Some(session_id) = req.session_id.clone() {
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
    let diff_root = session.working_dir.clone();
    let diff_baseline = workspace_change_snapshot(&diff_root).ok();
    tokio::spawn(async move {
        let mut session_for_task = session;
        session_for_task.conversation.push(user_message);

        while let Some(event) = core_events.recv().await {
            if let Some(message) = event
                .get("payload")
                .and_then(|payload| payload.get("message"))
                .cloned()
                .and_then(|value| serde_json::from_value::<Message>(value).ok())
            {
                session_for_task.conversation.push(message);
            }

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

        if !session_for_task
            .conversation
            .iter()
            .any(|message| message.role == Role::Assistant)
        {
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

fn normalize_permission_mode(mode: Option<String>) -> String {
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

fn sse_format_event(event: &serde_json::Value) -> String {
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
async fn reply(State(state): State<AppState>, Json(req): Json<ReplyRequest>) -> Response {
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
        // Auto-name the session from the first user message when it still has
        // a placeholder name, so the session list is human-readable.
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
    fn test_constant_time_eq_matching() {
        assert!(constant_time_eq(b"secret", b"secret"));
    }

    #[test]
    fn test_constant_time_eq_different() {
        assert!(!constant_time_eq(b"secret", b"secre7"));
        assert!(!constant_time_eq(b"secret", b"secret2"));
        assert!(!constant_time_eq(b"short", b"longer"));
    }

    #[test]
    fn test_is_public_path() {
        assert!(is_public_path("/healthz"));
        assert!(is_public_path("/readyz"));
        assert!(is_public_path("/swagger-ui"));
        assert!(is_public_path("/swagger-ui/"));
        assert!(is_public_path("/api-docs/openapi.json"));
        // Protected paths.
        assert!(!is_public_path("/reply"));
        assert!(!is_public_path("/sessions"));
        assert!(!is_public_path("/sessions/123/history"));
    }

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
    fn test_normalize_permission_mode() {
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
