use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::de::DeserializeOwned;
use tokio::sync::{mpsc, oneshot};
use tracing::warn;

use night24_protocol::{
    AgentToolsResult, CancelParams, Capability, InitializeEnvironment, InitializeParams,
    JsonRpcRequest, PeerInfo, PermissionDecision, PermissionResolution, ReplyAccepted, ReplyParams,
    SkillLoadParams, SkillLoadResult, SkillRegistryParams, SkillRegistryResult, SubAgentPoolParams,
    SubAgentPoolResult, PROTOCOL_VERSION,
};

type PendingResponses = Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>>;
type EventSenders = Arc<Mutex<HashMap<String, mpsc::Sender<serde_json::Value>>>>;

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
    pending: PendingResponses,
    event_senders: EventSenders,
    status: Arc<Mutex<CoreRuntimeStatus>>,
    generation: Arc<AtomicU64>,
}

impl AgentCoreClient {
    pub(crate) async fn spawn() -> anyhow::Result<Self> {
        let (stdin, child, stdout, stderr) = spawn_core_process()?;
        let generation = Arc::new(AtomicU64::new(1));

        let client = Self {
            stdin: Arc::new(Mutex::new(stdin)),
            child: Arc::new(Mutex::new(child)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            event_senders: Arc::new(Mutex::new(HashMap::new())),
            status: Arc::new(Mutex::new(CoreRuntimeStatus::unavailable("initializing"))),
            generation,
        };

        client.start_stdout_reader(stdout, 1);
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

    pub(crate) async fn restart(&self) -> anyhow::Result<CoreRuntimeStatus> {
        let generation_id = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut status) = self.status.lock() {
            *status = CoreRuntimeStatus::unavailable("restarting agent-core");
        }

        reject_pending_requests(&self.pending, "agent-core restarted");
        if let Ok(mut senders) = self.event_senders.lock() {
            senders.clear();
        }

        {
            let mut child = self
                .child
                .lock()
                .map_err(|_| anyhow::anyhow!("agent-core child lock poisoned"))?;
            let _ = child.kill();
            let _ = child.wait();
        }

        let (stdin, child, stdout, stderr) = match spawn_core_process() {
            Ok(parts) => parts,
            Err(err) => {
                if let Ok(mut status) = self.status.lock() {
                    *status =
                        CoreRuntimeStatus::unavailable(format!("agent-core restart failed: {err}"));
                }
                return Err(err);
            }
        };

        *self
            .stdin
            .lock()
            .map_err(|_| anyhow::anyhow!("agent-core stdin lock poisoned"))? = stdin;
        *self
            .child
            .lock()
            .map_err(|_| anyhow::anyhow!("agent-core child lock poisoned"))? = child;

        self.start_stdout_reader(stdout, generation_id);
        self.start_stderr_reader(stderr);
        if let Err(err) = self.initialize().await {
            if let Ok(mut status) = self.status.lock() {
                *status =
                    CoreRuntimeStatus::unavailable(format!("agent-core initialize failed: {err}"));
            }
            return Err(err);
        }

        let status = CoreRuntimeStatus::available();
        if let Ok(mut guard) = self.status.lock() {
            *guard = status.clone();
        }
        Ok(status)
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

    async fn call_typed<T: DeserializeOwned>(
        &self,
        method: &str,
        params: impl serde::Serialize,
        timeout: Duration,
    ) -> anyhow::Result<T> {
        let result = self.call(method, params, timeout).await?;
        serde_json::from_value(result)
            .map_err(|err| anyhow::anyhow!("agent-core {method} response decode failed: {err}"))
    }

    pub(crate) async fn tools(&self) -> anyhow::Result<AgentToolsResult> {
        self.call_typed(
            "agent.tools",
            serde_json::json!({ "include_disabled": false }),
            Duration::from_secs(5),
        )
        .await
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
            .call_typed::<ReplyAccepted>("agent.reply", params, Duration::from_secs(10))
            .await
        {
            Ok(accepted) => Ok((accepted, rx)),
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

    pub(crate) async fn subagents(
        &self,
        params: SubAgentPoolParams,
    ) -> anyhow::Result<SubAgentPoolResult> {
        self.call_typed("agent.subagents", params, Duration::from_secs(5))
            .await
    }

    pub(crate) async fn skills(
        &self,
        params: SkillRegistryParams,
    ) -> anyhow::Result<SkillRegistryResult> {
        self.call_typed("agent.skills", params, Duration::from_secs(5))
            .await
    }

    pub(crate) async fn load_skill(
        &self,
        params: SkillLoadParams,
    ) -> anyhow::Result<SkillLoadResult> {
        self.call_typed("agent.skill.load", params, Duration::from_secs(5))
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

    fn start_stdout_reader(&self, stdout: std::process::ChildStdout, generation_id: u64) {
        let pending = self.pending.clone();
        let event_senders = self.event_senders.clone();
        let status = self.status.clone();
        let generation = self.generation.clone();
        thread::spawn(move || {
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines() {
                let line = match line {
                    Ok(line) => line,
                    Err(err) => {
                        set_core_status_if_current(
                            &status,
                            &generation,
                            generation_id,
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

                match classify_core_stdout_line(&line) {
                    CoreStdoutMessage::Empty => continue,
                    CoreStdoutMessage::InvalidJson { error } => {
                        set_core_status_if_current(
                            &status,
                            &generation,
                            generation_id,
                            CoreRuntimeStatus::unavailable(format!(
                                "agent-core stdout protocol violation: {error}"
                            )),
                        );
                        continue;
                    }
                    CoreStdoutMessage::AgentEvent { params } => {
                        route_agent_event(&event_senders, params);
                    }
                    CoreStdoutMessage::JsonRpcResponse { id, value } => {
                        route_json_rpc_response(&pending, id, value);
                    }
                    CoreStdoutMessage::UnknownNotification { .. }
                    | CoreStdoutMessage::MalformedAgentEvent { .. }
                    | CoreStdoutMessage::UnknownMessage => {}
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

fn spawn_core_process() -> anyhow::Result<(
    ChildStdin,
    Child,
    std::process::ChildStdout,
    std::process::ChildStderr,
)> {
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

    Ok((stdin, child, stdout, stderr))
}

fn reject_pending_requests(pending: &PendingResponses, reason: &str) {
    let Ok(mut pending) = pending.lock() else {
        return;
    };
    for (id, tx) in pending.drain() {
        let _ = tx.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32003,
                "message": reason
            }
        }));
    }
}

#[derive(Debug, PartialEq)]
enum CoreStdoutMessage {
    Empty,
    InvalidJson {
        error: String,
    },
    JsonRpcResponse {
        id: String,
        value: serde_json::Value,
    },
    AgentEvent {
        params: serde_json::Value,
    },
    UnknownNotification {
        method: String,
    },
    MalformedAgentEvent {
        reason: &'static str,
    },
    UnknownMessage,
}

fn classify_core_stdout_line(line: &str) -> CoreStdoutMessage {
    if line.trim().is_empty() {
        return CoreStdoutMessage::Empty;
    }

    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(value) => classify_core_stdout_value(value),
        Err(err) => CoreStdoutMessage::InvalidJson {
            error: err.to_string(),
        },
    }
}

fn classify_core_stdout_value(value: serde_json::Value) -> CoreStdoutMessage {
    if json_rpc_method(&value) == Some("agent.event") {
        return classify_agent_event_notification(value);
    }

    if let Some(id) = value.get("id").and_then(json_rpc_id_key) {
        return CoreStdoutMessage::JsonRpcResponse { id, value };
    }

    if let Some(method) = json_rpc_method(&value).map(str::to_string) {
        return CoreStdoutMessage::UnknownNotification { method };
    }

    CoreStdoutMessage::UnknownMessage
}

fn classify_agent_event_notification(value: serde_json::Value) -> CoreStdoutMessage {
    let Some(params) = value.get("params").cloned() else {
        return CoreStdoutMessage::MalformedAgentEvent {
            reason: "missing params",
        };
    };

    if agent_event_run_id(&params).is_none() {
        return CoreStdoutMessage::MalformedAgentEvent {
            reason: "missing run_id",
        };
    }

    CoreStdoutMessage::AgentEvent { params }
}

fn route_agent_event(event_senders: &EventSenders, params: serde_json::Value) {
    let Some(run_id) = agent_event_run_id(&params).map(str::to_string) else {
        return;
    };

    let is_terminal = is_terminal_agent_event(&params);

    let sender = event_senders
        .lock()
        .ok()
        .and_then(|guard| guard.get(&run_id).cloned());
    if let Some(sender) = sender {
        if sender.blocking_send(params).is_err() {
            if let Ok(mut guard) = event_senders.lock() {
                guard.remove(&run_id);
            }
            return;
        }
    }
    if is_terminal {
        if let Ok(mut guard) = event_senders.lock() {
            guard.remove(&run_id);
        }
    }
}

fn route_json_rpc_response(pending: &PendingResponses, id: String, value: serde_json::Value) {
    let tx = pending.lock().ok().and_then(|mut guard| guard.remove(&id));
    if let Some(tx) = tx {
        let _ = tx.send(value);
    }
}

fn agent_event_run_id(event: &serde_json::Value) -> Option<&str> {
    event.get("run_id").and_then(|run_id| run_id.as_str())
}

fn is_terminal_agent_event(event: &serde_json::Value) -> bool {
    matches!(
        event.get("type").and_then(|kind| kind.as_str()),
        Some("finish" | "error")
    )
}

fn set_core_status_if_current(
    status_ptr: &Arc<Mutex<CoreRuntimeStatus>>,
    generation: &Arc<AtomicU64>,
    generation_id: u64,
    status: CoreRuntimeStatus,
) {
    if generation.load(Ordering::SeqCst) != generation_id {
        return;
    }
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

fn json_rpc_method(value: &serde_json::Value) -> Option<&str> {
    value.get("method").and_then(|method| method.as_str())
}

fn locate_agent_core_bin() -> PathBuf {
    if let Ok(path) = std::env::var("NIGHT24_AGENT_CORE_BIN") {
        if let Some(path) = non_empty_path(path) {
            return path;
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

fn non_empty_path(value: impl AsRef<str>) -> Option<PathBuf> {
    let value = value.as_ref().trim();
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_agent_event_notifications() {
        let event = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "agent.event",
            "params": { "run_id": "run-1", "type": "message" }
        });

        match classify_core_stdout_value(event) {
            CoreStdoutMessage::AgentEvent { params } => {
                assert_eq!(params["run_id"], "run-1");
                assert_eq!(params["type"], "message");
            }
            other => panic!("expected agent event, got {other:?}"),
        }
    }

    #[test]
    fn classifies_json_rpc_responses() {
        let response = serde_json::json!({ "jsonrpc": "2.0", "id": "rpc-1", "result": {} });

        match classify_core_stdout_value(response) {
            CoreStdoutMessage::JsonRpcResponse { id, value } => {
                assert_eq!(id, "rpc-1");
                assert_eq!(value["result"], serde_json::json!({}));
            }
            other => panic!("expected json-rpc response, got {other:?}"),
        }
    }

    #[test]
    fn classifies_numeric_json_rpc_response_ids() {
        let response = serde_json::json!({ "jsonrpc": "2.0", "id": 42, "result": true });

        match classify_core_stdout_value(response) {
            CoreStdoutMessage::JsonRpcResponse { id, value } => {
                assert_eq!(id, "42");
                assert_eq!(value["result"], true);
            }
            other => panic!("expected json-rpc response, got {other:?}"),
        }
    }

    #[test]
    fn ignores_json_rpc_messages_without_string_or_integer_id() {
        for response in [
            serde_json::json!({ "jsonrpc": "2.0", "id": null, "result": true }),
            serde_json::json!({ "jsonrpc": "2.0", "id": { "nested": true }, "result": true }),
            serde_json::json!({ "jsonrpc": "2.0", "id": 1.5, "result": true }),
        ] {
            assert_eq!(
                classify_core_stdout_value(response),
                CoreStdoutMessage::UnknownMessage
            );
        }
    }

    #[test]
    fn classifies_unknown_notifications_and_malformed_agent_events() {
        let unknown = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "core.telemetry",
            "params": { "ok": true }
        });
        assert_eq!(
            classify_core_stdout_value(unknown),
            CoreStdoutMessage::UnknownNotification {
                method: "core.telemetry".to_string()
            }
        );

        let non_string_method = serde_json::json!({
            "jsonrpc": "2.0",
            "method": 7,
            "params": { "ok": true }
        });
        assert_eq!(
            classify_core_stdout_value(non_string_method),
            CoreStdoutMessage::UnknownMessage
        );

        let malformed = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "agent.event",
            "params": { "type": "message" }
        });
        assert_eq!(
            classify_core_stdout_value(malformed),
            CoreStdoutMessage::MalformedAgentEvent {
                reason: "missing run_id"
            }
        );

        let missing_params = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "agent.event"
        });
        assert_eq!(
            classify_core_stdout_value(missing_params),
            CoreStdoutMessage::MalformedAgentEvent {
                reason: "missing params"
            }
        );
    }

    #[test]
    fn classifies_invalid_json_lines() {
        match classify_core_stdout_line("{not json") {
            CoreStdoutMessage::InvalidJson { error } => assert!(!error.is_empty()),
            other => panic!("expected invalid json, got {other:?}"),
        }
    }

    #[test]
    fn classifies_blank_stdout_lines_without_json_parse_error() {
        assert_eq!(classify_core_stdout_line(""), CoreStdoutMessage::Empty);
        assert_eq!(classify_core_stdout_line(" \t "), CoreStdoutMessage::Empty);
    }

    #[test]
    fn detects_terminal_agent_events() {
        assert!(is_terminal_agent_event(
            &serde_json::json!({ "type": "finish" })
        ));
        assert!(is_terminal_agent_event(
            &serde_json::json!({ "type": "error" })
        ));
        assert!(!is_terminal_agent_event(
            &serde_json::json!({ "type": "message" })
        ));
        assert!(!is_terminal_agent_event(&serde_json::json!({})));
    }

    #[test]
    fn extracts_agent_event_run_id_only_when_string() {
        assert_eq!(
            agent_event_run_id(&serde_json::json!({ "run_id": "run-1" })),
            Some("run-1")
        );
        assert_eq!(
            agent_event_run_id(&serde_json::json!({ "run_id": 7 })),
            None
        );
        assert_eq!(agent_event_run_id(&serde_json::json!({})), None);
    }

    #[test]
    fn route_agent_event_removes_sender_after_terminal_event() {
        let event_senders = Arc::new(Mutex::new(HashMap::new()));
        let (tx, mut rx) = mpsc::channel(1);
        event_senders
            .lock()
            .unwrap()
            .insert("run-1".to_string(), tx);

        route_agent_event(
            &event_senders,
            serde_json::json!({ "run_id": "run-1", "type": "finish" }),
        );

        assert!(event_senders.lock().unwrap().get("run-1").is_none());
        let routed = rx.blocking_recv().unwrap();
        assert_eq!(routed["type"], "finish");
    }

    #[test]
    fn route_agent_event_keeps_sender_for_non_terminal_event() {
        let event_senders = Arc::new(Mutex::new(HashMap::new()));
        let (tx, mut rx) = mpsc::channel(1);
        event_senders
            .lock()
            .unwrap()
            .insert("run-1".to_string(), tx);

        route_agent_event(
            &event_senders,
            serde_json::json!({ "run_id": "run-1", "type": "message" }),
        );

        assert!(event_senders.lock().unwrap().get("run-1").is_some());
        let routed = rx.blocking_recv().unwrap();
        assert_eq!(routed["type"], "message");
    }

    #[test]
    fn route_agent_event_ignores_events_without_known_sender() {
        let event_senders = Arc::new(Mutex::new(HashMap::new()));

        route_agent_event(
            &event_senders,
            serde_json::json!({ "run_id": "missing", "type": "finish" }),
        );
        route_agent_event(&event_senders, serde_json::json!({ "type": "finish" }));

        assert!(event_senders.lock().unwrap().is_empty());
    }

    #[test]
    fn route_agent_event_removes_terminal_sender_even_when_receiver_closed() {
        let event_senders = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        event_senders
            .lock()
            .unwrap()
            .insert("run-1".to_string(), tx);

        route_agent_event(
            &event_senders,
            serde_json::json!({ "run_id": "run-1", "type": "error" }),
        );

        assert!(event_senders.lock().unwrap().get("run-1").is_none());
    }

    #[test]
    fn route_agent_event_removes_closed_sender_for_non_terminal_event() {
        let event_senders = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        event_senders
            .lock()
            .unwrap()
            .insert("run-1".to_string(), tx);

        route_agent_event(
            &event_senders,
            serde_json::json!({ "run_id": "run-1", "type": "message" }),
        );

        assert!(event_senders.lock().unwrap().get("run-1").is_none());
    }

    #[test]
    fn route_json_rpc_response_resolves_and_removes_pending_request() {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = oneshot::channel();
        pending.lock().unwrap().insert("rpc-1".to_string(), tx);

        route_json_rpc_response(
            &pending,
            "rpc-1".to_string(),
            serde_json::json!({ "result": true }),
        );

        assert!(pending.lock().unwrap().is_empty());
        assert_eq!(rx.blocking_recv().unwrap()["result"], true);
    }

    #[test]
    fn route_json_rpc_response_ignores_unknown_pending_id() {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (tx, mut rx) = oneshot::channel();
        pending.lock().unwrap().insert("rpc-1".to_string(), tx);

        route_json_rpc_response(
            &pending,
            "rpc-missing".to_string(),
            serde_json::json!({ "result": true }),
        );

        assert!(pending.lock().unwrap().contains_key("rpc-1"));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn route_json_rpc_response_removes_pending_even_when_receiver_closed() {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = oneshot::channel();
        drop(rx);
        pending.lock().unwrap().insert("rpc-1".to_string(), tx);

        route_json_rpc_response(
            &pending,
            "rpc-1".to_string(),
            serde_json::json!({ "result": true }),
        );

        assert!(pending.lock().unwrap().is_empty());
    }

    #[test]
    fn non_empty_path_ignores_blank_values() {
        assert_eq!(non_empty_path(""), None);
        assert_eq!(non_empty_path("   "), None);
        assert_eq!(
            non_empty_path(" target/debug/night24-agent-core "),
            Some(PathBuf::from("target/debug/night24-agent-core"))
        );
    }
}
