use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, Method},
    response::{IntoResponse, Response},
    routing::get,
    routing::post,
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

#[derive(Debug, Serialize, utoipa::ToSchema)]
struct SessionSummary {
    id: String,
    name: String,
    session_type: String,
    updated_at: String,
}

#[derive(OpenApi)]
#[openapi(
    paths(healthz, reply, list_sessions, get_session_history, create_session),
    components(schemas(ReplyRequest, CreateSessionRequest, SessionSummary)),
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
    let provider_registry = ProviderRegistry::new("echo")
        .with_echo()
        .with_openai(
            std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            "https://api.openai.com/v1",
            "gpt-4o-mini",
        )
        .with_anthropic(
            "6s0zttvJqZB1ZS9lWcWmv9IfVBVqJ0izrCthf0fUQFvXgDbii0L9a1A46zlX0nuox",
            "https://api.stepfun.com/step_plan/v1",
            "step-3.7-flash",
        )
        .with_stepfun(
            "6s0zttvJqZB1ZS9lWcWmv9IfVBVqJ0izrCthf0fUQFvXgDbii0L9a1A46zlX0nuox",
            "https://api.stepfun.com/step_plan/v1",
            "step-3.7-flash",
        )
        .with_ollama("http://localhost:11434", "llama3.2");

    let app_state = AppState {
        session_manager,
        provider_registry: Arc::new(provider_registry),
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/reply", post(reply))
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{id}/history", get(get_session_history))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]),
        )
        .with_state(app_state);

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

    let agent = Agent::new(
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
