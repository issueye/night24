use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{header, Method},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
    routing::post,
    routing::put,
    Json, Router,
};
use futures::stream;
use serde::{Deserialize, Serialize};
use tokio::signal;
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
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
struct CreateSessionRequest {
    #[schema(example = "my-chat")]
    name: Option<String>,
    #[schema(example = "user")]
    session_type: Option<String>,
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
    updated_at: String,
}

#[derive(OpenApi)]
#[openapi(
    paths(healthz, reply, list_sessions, get_session_history, create_session, rename_session, fork_session),
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
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
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

    let session_manager = if let Ok(db_url) = std::env::var("NIGHT24_DATABASE_URL") {
        let db_url = db_url.trim().to_string();
        let db_url = if db_url == "sqlite::memory:" || db_url.starts_with("sqlite:file:") {
            db_url
        } else if db_url.starts_with("sqlite:") {
            let path = db_url.trim_start_matches("sqlite:").trim_start_matches('/');
            format!("sqlite:file:{}?mode=rwc", path)
        } else {
            format!("sqlite:file:{}?mode=rwc", db_url)
        };
        info!(db_url, "initializing sqlite session store");
        match SessionManager::with_sqlite(db_url).await {
            Ok(manager) => Arc::new(manager),
            Err(err) => {
                warn!(error = ?err, "failed to init sqlite session store, falling back to in-memory");
                Arc::new(SessionManager::new())
            }
        }
    } else {
        Arc::new(SessionManager::new())
    };
    let provider_registry = build_provider_registry();
    let permission_manager = build_permission_manager();

    let app_state = AppState {
        session_manager,
        provider_registry: Arc::new(provider_registry),
        permission_manager: Arc::new(permission_manager),
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/reply", post(reply))
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{id}/history", get(get_session_history))
        .route("/sessions/{id}/name", put(rename_session))
        .route("/sessions/{id}/fork", post(fork_session))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([Method::GET, Method::POST, Method::PUT])
                .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]),
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
            let model = std::env::var("OPENAI_MODEL")
                .unwrap_or_else(|_| "gpt-4o-mini".to_string());
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
            let model = std::env::var("STEPFUN_MODEL")
                .unwrap_or_else(|_| "step-3.7-flash".to_string());
            registry = registry.with_stepfun(api_key, base_url, model);
        }
    }

    let ollama_base_url = std::env::var("OLLAMA_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());
    let ollama_model = std::env::var("OLLAMA_MODEL")
        .unwrap_or_else(|_| "llama3.2".to_string());
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
            info!(permission_mode = "strict", "permission: strict (confirm all)");
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
struct AppState {
    session_manager: Arc<SessionManager>,
    provider_registry: Arc<ProviderRegistry>,
    permission_manager: Arc<night24_core::permission::PermissionManager>,
}

/// Whether a request path is exempt from authentication (health/docs).
fn is_public_path(path: &str) -> bool {
    path == "/healthz" || path.starts_with("/swagger-ui") || path.starts_with("/api-docs")
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
    path = "/sessions",
    tag = "night24",
    responses(
        (status = 200, description = "List all sessions", body = Vec<SessionSummary>)
    )
)]
async fn list_sessions(
    State(state): State<AppState>,
) -> Result<Json<Vec<SessionSummary>>, axum::http::StatusCode> {
    let sessions = state.session_manager.list().await.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let summaries = sessions
        .into_iter()
        .map(|s| SessionSummary {
            id: s.id,
            name: s.name,
            session_type: format!("{:?}", s.session_type),
            updated_at: s.updated_at.to_rfc3339(),
        })
        .collect();
    Ok(Json(summaries))
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
        .create(name, PathBuf::from("."), session_type)
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
    let session = state.session_manager.get(&id).await.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
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

#[utoipa::path(
    post,
    path = "/reply",
    tag = "night24",
    request_body = ReplyRequest,
    responses(
        (status = 200, description = "SSE stream of agent messages", content_type = "text/event-stream")
    )
)]
async fn reply(
    State(state): State<AppState>,
    Json(req): Json<ReplyRequest>,
) -> Response {
    let provider_name = req.provider.as_deref().unwrap_or("echo");
    let provider: Arc<dyn night24_core::provider::Provider> = if provider_name == "openai" {
        let api_key = req.api_key.unwrap_or_else(|| {
            std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| "".to_string())
        });
        if api_key.is_empty() {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "api_key is required for openai provider"})),
            )
                .into_response();
        }
        state.provider_registry.create_with_model("openai", req.model.clone().unwrap_or_else(|| "gpt-4o-mini".to_string()))
    } else if provider_name == "anthropic" {
        state.provider_registry.create_with_model("anthropic", req.model.clone().unwrap_or_else(|| "step-3.7-flash".to_string()))
    } else if provider_name == "ollama" {
        state.provider_registry.create_with_model("ollama", req.model.clone().unwrap_or_else(|| "llama3.2".to_string()))
    } else if provider_name == "stepfun" {
        state.provider_registry.create_with_model("stepfun", req.model.clone().unwrap_or_else(|| "step-3.7-flash".to_string()))
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
                    Json(serde_json::json!({"error": format!("session not found: {}", session_id)})),
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
        state
            .session_manager
            .create("session", PathBuf::from("."), SessionType::User)
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
            max_turns: 10,
            turn_timeout: Duration::from_secs(60),
            tool_timeout: Duration::from_secs(30),
            total_timeout: Duration::from_secs(180),
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

    let stream = stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Some(Ok(m)) => {
                let json = serde_json::to_string(&m).unwrap_or_default();
                Some((
                    Ok::<String, std::convert::Infallible>(format!("data: {}\n\n", json)),
                    rx,
                ))
            }
            Some(Err(e)) => {
                Some((
                    Ok::<String, std::convert::Infallible>(format!("data: error: {}\n\n", e)),
                    rx,
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
        assert!(is_public_path("/swagger-ui"));
        assert!(is_public_path("/swagger-ui/"));
        assert!(is_public_path("/api-docs/openapi.json"));
        // Protected paths.
        assert!(!is_public_path("/reply"));
        assert!(!is_public_path("/sessions"));
        assert!(!is_public_path("/sessions/123/history"));
    }
}
