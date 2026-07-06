use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use tokio::sync::{mpsc, Mutex, OwnedSemaphorePermit, RwLock, Semaphore};

use night24_protocol::{PermissionDecision, ReplyAccepted, ReplyParams};

use crate::core_client::AgentCoreClient;

const DEFAULT_MAX_PROCESSES: usize = 4;

pub(crate) struct RunStart {
    pub(crate) accepted: ReplyAccepted,
    pub(crate) events: mpsc::Receiver<serde_json::Value>,
}

pub(crate) trait AgentRunner: Send + Sync {
    fn start_reply(&self, params: ReplyParams) -> BoxFuture<'_, anyhow::Result<RunStart>>;

    fn cancel(
        &self,
        run_id: String,
        reason: Option<String>,
    ) -> BoxFuture<'_, anyhow::Result<serde_json::Value>>;

    fn resolve_permission(
        &self,
        run_id: String,
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
    ) -> BoxFuture<'_, anyhow::Result<serde_json::Value>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunnerMode {
    SingleCore,
    PerRunProcess,
    ProcessPool,
}

impl RunnerMode {
    pub(crate) fn from_env() -> Self {
        Self::parse(std::env::var("NIGHT24_AGENT_RUNNER").ok().as_deref())
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::SingleCore => "single_core",
            Self::PerRunProcess => "per_run_process",
            Self::ProcessPool => "process_pool",
        }
    }

    pub(crate) fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            Some("per_run_process") => Self::PerRunProcess,
            Some("process_pool") => Self::ProcessPool,
            Some("single_core") | None => Self::SingleCore,
            Some(_) => Self::SingleCore,
        }
    }
}

pub(crate) fn build_agent_runner(
    mode: RunnerMode,
    core_client: Arc<RwLock<Option<Arc<AgentCoreClient>>>>,
) -> Arc<dyn AgentRunner> {
    match mode {
        RunnerMode::SingleCore => Arc::new(SingleCoreRunner::new(core_client)),
        RunnerMode::PerRunProcess => Arc::new(PerRunProcessRunner::from_env()),
        RunnerMode::ProcessPool => Arc::new(UnsupportedRunner::new("process_pool")),
    }
}

pub(crate) struct SingleCoreRunner {
    core_client: Arc<RwLock<Option<Arc<AgentCoreClient>>>>,
}

impl SingleCoreRunner {
    pub(crate) fn new(core_client: Arc<RwLock<Option<Arc<AgentCoreClient>>>>) -> Self {
        Self { core_client }
    }

    async fn current_core_client(&self) -> anyhow::Result<Arc<AgentCoreClient>> {
        self.core_client
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no active core client"))
    }
}

impl AgentRunner for SingleCoreRunner {
    fn start_reply(&self, params: ReplyParams) -> BoxFuture<'_, anyhow::Result<RunStart>> {
        Box::pin(async move {
            let core_client = self.current_core_client().await?;
            let (accepted, events) = core_client.reply(params).await?;
            Ok(RunStart { accepted, events })
        })
    }

    fn cancel(
        &self,
        run_id: String,
        reason: Option<String>,
    ) -> BoxFuture<'_, anyhow::Result<serde_json::Value>> {
        Box::pin(async move {
            let core_client = self.current_core_client().await?;
            core_client.cancel(run_id, reason).await
        })
    }

    fn resolve_permission(
        &self,
        run_id: String,
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
    ) -> BoxFuture<'_, anyhow::Result<serde_json::Value>> {
        Box::pin(async move {
            let core_client = self.current_core_client().await?;
            core_client
                .resolve_permission(run_id, permission_id, decision, reason)
                .await
        })
    }
}

pub(crate) struct PerRunProcessRunner {
    active_runs: Arc<Mutex<HashMap<String, ActiveRunProcess>>>,
    permits: Arc<Semaphore>,
    max_processes: usize,
}

struct ActiveRunProcess {
    client: Arc<AgentCoreClient>,
    _permit: OwnedSemaphorePermit,
}

impl PerRunProcessRunner {
    pub(crate) fn from_env() -> Self {
        Self::with_max_processes(max_processes_from_env())
    }

    pub(crate) fn with_max_processes(max_processes: usize) -> Self {
        Self {
            active_runs: Arc::new(Mutex::new(HashMap::new())),
            permits: Arc::new(Semaphore::new(max_processes)),
            max_processes,
        }
    }

    async fn active_client(&self, run_id: &str) -> anyhow::Result<Arc<AgentCoreClient>> {
        self.active_runs
            .lock()
            .await
            .get(run_id)
            .map(|entry| entry.client.clone())
            .ok_or_else(|| anyhow::anyhow!("no active agent process for run_id {run_id}"))
    }
}

impl AgentRunner for PerRunProcessRunner {
    fn start_reply(&self, params: ReplyParams) -> BoxFuture<'_, anyhow::Result<RunStart>> {
        Box::pin(async move {
            if self.max_processes == 0 {
                return Err(process_limit_reached(self.max_processes));
            }

            let permit = self
                .permits
                .clone()
                .try_acquire_owned()
                .map_err(|_| process_limit_reached(self.max_processes))?;

            let run_id = params.run_id.clone();
            let client = Arc::new(AgentCoreClient::spawn().await?);
            let (accepted, core_events) = client.reply(params).await?;
            let (tx, events) = mpsc::channel(64);

            self.active_runs.lock().await.insert(
                run_id.clone(),
                ActiveRunProcess {
                    client,
                    _permit: permit,
                },
            );

            tokio::spawn(forward_run_events(
                run_id,
                core_events,
                tx,
                self.active_runs.clone(),
            ));

            Ok(RunStart { accepted, events })
        })
    }

    fn cancel(
        &self,
        run_id: String,
        reason: Option<String>,
    ) -> BoxFuture<'_, anyhow::Result<serde_json::Value>> {
        Box::pin(async move {
            let client = self.active_client(&run_id).await?;
            client.cancel(run_id, reason).await
        })
    }

    fn resolve_permission(
        &self,
        run_id: String,
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
    ) -> BoxFuture<'_, anyhow::Result<serde_json::Value>> {
        Box::pin(async move {
            let client = self.active_client(&run_id).await?;
            client
                .resolve_permission(run_id, permission_id, decision, reason)
                .await
        })
    }
}

async fn forward_run_events(
    run_id: String,
    mut core_events: mpsc::Receiver<serde_json::Value>,
    tx: mpsc::Sender<serde_json::Value>,
    active_runs: Arc<Mutex<HashMap<String, ActiveRunProcess>>>,
) {
    while let Some(event) = core_events.recv().await {
        let is_terminal = is_terminal_agent_event(&event);
        let _ = tx.send(event).await;
        if is_terminal {
            break;
        }
    }

    active_runs.lock().await.remove(&run_id);
}

struct UnsupportedRunner {
    mode: &'static str,
}

impl UnsupportedRunner {
    fn new(mode: &'static str) -> Self {
        Self { mode }
    }

    fn unsupported(&self) -> anyhow::Error {
        anyhow::anyhow!("agent runner mode {} is not implemented yet", self.mode)
    }
}

impl AgentRunner for UnsupportedRunner {
    fn start_reply(&self, _params: ReplyParams) -> BoxFuture<'_, anyhow::Result<RunStart>> {
        Box::pin(async move { Err(self.unsupported()) })
    }

    fn cancel(
        &self,
        _run_id: String,
        _reason: Option<String>,
    ) -> BoxFuture<'_, anyhow::Result<serde_json::Value>> {
        Box::pin(async move { Err(self.unsupported()) })
    }

    fn resolve_permission(
        &self,
        _run_id: String,
        _permission_id: String,
        _decision: PermissionDecision,
        _reason: Option<String>,
    ) -> BoxFuture<'_, anyhow::Result<serde_json::Value>> {
        Box::pin(async move { Err(self.unsupported()) })
    }
}

fn max_processes_from_env() -> usize {
    std::env::var("NIGHT24_AGENT_MAX_PROCESSES")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_PROCESSES)
}

fn process_limit_reached(max_processes: usize) -> anyhow::Error {
    anyhow::anyhow!("agent process limit reached: max active processes is {max_processes}")
}

fn is_terminal_agent_event(event: &serde_json::Value) -> bool {
    matches!(
        event.get("type").and_then(|kind| kind.as_str()),
        Some("finish" | "error")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use night24_protocol::{ProviderConfig, ReplyInput, ReplyLimits, ReplyOptions, ReplySession};

    #[test]
    fn runner_mode_parse_defaults_to_single_core() {
        assert_eq!(RunnerMode::parse(None), RunnerMode::SingleCore);
        assert_eq!(RunnerMode::parse(Some("")), RunnerMode::SingleCore);
        assert_eq!(
            RunnerMode::parse(Some("single_core")),
            RunnerMode::SingleCore
        );
        assert_eq!(
            RunnerMode::parse(Some("per_run_process")),
            RunnerMode::PerRunProcess
        );
        assert_eq!(
            RunnerMode::parse(Some("process_pool")),
            RunnerMode::ProcessPool
        );
        assert_eq!(RunnerMode::parse(Some("unknown")), RunnerMode::SingleCore);
    }

    #[test]
    fn terminal_agent_event_detection_matches_core_contract() {
        assert!(is_terminal_agent_event(
            &serde_json::json!({ "type": "finish" })
        ));
        assert!(is_terminal_agent_event(
            &serde_json::json!({ "type": "error" })
        ));
        assert!(!is_terminal_agent_event(
            &serde_json::json!({ "type": "message" })
        ));
    }

    #[tokio::test]
    async fn per_run_max_process_guard_rejects_when_no_capacity() {
        let runner = PerRunProcessRunner::with_max_processes(0);

        let err = match runner.start_reply(reply_params("run-guard")).await {
            Ok(_) => panic!("process guard should reject before spawning"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("agent process limit reached"));
    }

    fn reply_params(run_id: &str) -> ReplyParams {
        ReplyParams {
            run_id: run_id.to_string(),
            session: ReplySession {
                id: "session-test".to_string(),
                name: "session".to_string(),
                working_dir: std::path::PathBuf::from("."),
                conversation: Vec::new(),
            },
            input: ReplyInput {
                text: "hello".to_string(),
            },
            provider: ProviderConfig {
                provider: "echo".to_string(),
                model: "echo-v1".to_string(),
                base_url: None,
                api_key_ref: None,
                api_key: None,
            },
            limits: ReplyLimits::default(),
            options: ReplyOptions {
                stream_message_delta: true,
                emit_tool_events: true,
                permission_mode: Some("strict".to_string()),
                network_proxy: None,
                context_threshold_tokens: None,
            },
        }
    }
}
