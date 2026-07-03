use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tracing::warn;

use night24_protocol::{
    AgentToolsResult, CancelParams, Capability, InitializeEnvironment, InitializeParams,
    JsonRpcRequest, PeerInfo, PermissionDecision, PermissionResolution, ReplyAccepted, ReplyParams,
    PROTOCOL_VERSION,
};

#[derive(Debug, Clone)]
pub(crate) struct CoreRuntimeStatus {
    pub(crate) available: bool,
    pub(crate) initialized: bool,
    pub(crate) reason: Option<String>,
}

impl CoreRuntimeStatus {
    fn available() -> Self {
        Self {
            available: true,
            initialized: true,
            reason: None,
        }
    }

    pub(crate) fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            initialized: false,
            reason: Some(reason.into()),
        }
    }
}

pub(crate) struct AgentCoreClient {
    stdin: Arc<Mutex<ChildStdin>>,
    child: Arc<Mutex<Child>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>>,
    event_senders: Arc<Mutex<HashMap<String, mpsc::Sender<serde_json::Value>>>>,
    status: Arc<Mutex<CoreRuntimeStatus>>,
}

impl AgentCoreClient {
    pub(crate) async fn spawn() -> anyhow::Result<Self> {
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

    pub(crate) async fn status(&self) -> CoreRuntimeStatus {
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

    pub(crate) async fn tools(&self) -> anyhow::Result<AgentToolsResult> {
        let result = self
            .call(
                "agent.tools",
                serde_json::json!({ "include_disabled": false }),
                Duration::from_secs(5),
            )
            .await?;
        serde_json::from_value(result).map_err(Into::into)
    }

    pub(crate) async fn reply(
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

    pub(crate) async fn cancel(
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

    pub(crate) async fn resolve_permission(
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
