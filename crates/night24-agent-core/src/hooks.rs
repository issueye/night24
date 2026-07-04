use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use gts::runtime::{RunOptions, Session};
use night24_protocol::OutputStream;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

const DEFAULT_TIMEOUT_MS: u64 = 5_000;
const MAX_OUTPUT_CHARS: usize = 8_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum HookEvent {
    RunStarted,
    BeforeProviderRequest,
    BeforeTool,
    AfterTool,
    PermissionRequired,
    RunFinished,
    RunFailed,
}

impl HookEvent {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::RunStarted => "run_started",
            Self::BeforeProviderRequest => "before_provider_request",
            Self::BeforeTool => "before_tool",
            Self::AfterTool => "after_tool",
            Self::PermissionRequired => "permission_required",
            Self::RunFinished => "run_finished",
            Self::RunFailed => "run_failed",
        }
    }
}

impl fmt::Display for HookEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub(super) struct HookRunner {
    hooks: Vec<HookDefinition>,
    gts_worker: Arc<GtsHookWorker>,
}

impl Default for HookRunner {
    fn default() -> Self {
        Self {
            hooks: Vec::new(),
            gts_worker: Arc::new(GtsHookWorker::new()),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct HookConfig {
    #[serde(default)]
    hooks: Vec<HookDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
struct HookDefinition {
    event: HookEvent,
    #[serde(default)]
    script: Option<PathBuf>,
    #[serde(default)]
    inline_script: Option<String>,
    #[serde(default)]
    engine: Option<HookEngine>,
    #[serde(default)]
    instruction_limit: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum HookEngine {
    #[serde(rename = "gts", alias = "goscript")]
    Gts,
}

impl fmt::Display for HookEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gts => f.write_str("gts"),
        }
    }
}

#[derive(Debug)]
pub(super) struct HookContext<'a> {
    pub(super) event: HookEvent,
    pub(super) run_id: &'a str,
    pub(super) working_dir: &'a Path,
    pub(super) provider: Option<&'a str>,
    pub(super) model: Option<&'a str>,
    pub(super) message_count: Option<usize>,
    pub(super) tool_count: Option<usize>,
    pub(super) tool_call_id: Option<&'a str>,
    pub(super) tool_name: Option<&'a str>,
    pub(super) summary: Option<&'a str>,
    pub(super) arguments: Option<&'a serde_json::Value>,
    pub(super) result_preview: Option<&'a str>,
    pub(super) error: Option<&'a str>,
    pub(super) duration_ms: Option<u64>,
    pub(super) finish_status: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct SerializableHookContext<'a> {
    event: &'static str,
    run_id: &'a str,
    working_dir: String,
    provider: Option<&'a str>,
    model: Option<&'a str>,
    message_count: Option<usize>,
    tool_count: Option<usize>,
    tool_call_id: Option<&'a str>,
    tool_name: Option<&'a str>,
    summary: Option<&'a str>,
    arguments: Option<&'a serde_json::Value>,
    result_preview: Option<&'a str>,
    error: Option<&'a str>,
    duration_ms: Option<u64>,
    finish_status: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(super) struct HookOutput {
    pub(super) source: String,
    pub(super) stream: OutputStream,
    pub(super) text: String,
}

impl HookRunner {
    pub(super) fn from_environment(working_dir: &Path) -> Self {
        let Some(path) = hook_config_path(working_dir) else {
            return Self::default();
        };

        match Self::from_path(&path) {
            Ok(runner) => runner,
            Err(err) => {
                eprintln!("failed to load hook config {}: {err}", path.display());
                Self::default()
            }
        }
    }

    fn from_path(path: &Path) -> anyhow::Result<Self> {
        Self::from_config_str(&std::fs::read_to_string(path)?)
    }

    pub(super) fn from_config_str(config: &str) -> anyhow::Result<Self> {
        let config: HookConfig = serde_json::from_str(config)?;
        Ok(Self {
            hooks: config
                .hooks
                .into_iter()
                .filter(|hook| hook.enabled && hook.has_executor())
                .collect(),
            gts_worker: Arc::new(GtsHookWorker::new()),
        })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    pub(super) async fn run(&self, context: &HookContext<'_>) -> Vec<HookOutput> {
        if self.is_empty() {
            return Vec::new();
        }

        let mut outputs = Vec::new();
        for hook in self.hooks.iter().filter(|hook| hook.event == context.event) {
            outputs.extend(run_gts_hook(hook, context, &self.gts_worker).await);
        }
        outputs
    }
}

fn hook_config_path(working_dir: &Path) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("NIGHT24_HOOKS_FILE")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return Some(if path.is_absolute() {
            path
        } else {
            working_dir.join(path)
        });
    }

    let workspace_config = working_dir.join(".night24").join("hooks.json");
    if workspace_config.is_file() {
        Some(workspace_config)
    } else {
        None
    }
}

async fn run_gts_hook(
    hook: &HookDefinition,
    context: &HookContext<'_>,
    worker: &GtsHookWorker,
) -> Vec<HookOutput> {
    let source = hook_source(hook, context.event);
    let timeout = Duration::from_millis(hook.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS).max(1));
    let request = match GtsHookRequest::from_hook(hook, context, timeout) {
        Ok(request) => request,
        Err(err) => {
            return vec![HookOutput {
                source,
                stream: OutputStream::Stderr,
                text: err.to_string(),
            }]
        }
    };

    match worker.run(request).await {
        Ok(outputs) => outputs
            .into_iter()
            .map(|mut output| {
                output.source = source.clone();
                output
            })
            .collect(),
        Err(err) => vec![HookOutput {
            source,
            stream: OutputStream::Stderr,
            text: err,
        }],
    }
}

pub(super) fn hook_context_json(context: &HookContext<'_>) -> String {
    serde_json::to_string(&SerializableHookContext {
        event: context.event.as_str(),
        run_id: context.run_id,
        working_dir: context.working_dir.to_string_lossy().to_string(),
        provider: context.provider,
        model: context.model,
        message_count: context.message_count,
        tool_count: context.tool_count,
        tool_call_id: context.tool_call_id,
        tool_name: context.tool_name,
        summary: context.summary,
        arguments: context.arguments,
        result_preview: context.result_preview,
        error: context.error,
        duration_ms: context.duration_ms,
        finish_status: context.finish_status,
    })
    .unwrap_or_else(|err| {
        serde_json::json!({
            "error": format!("failed to serialize hook context: {err}")
        })
        .to_string()
    })
}

fn hook_source(hook: &HookDefinition, event: HookEvent) -> String {
    let name = hook
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("gts");
    format!("hook:{event}:{name}")
}

impl HookDefinition {
    fn has_executor(&self) -> bool {
        let _engine = self.engine.unwrap_or(HookEngine::Gts);
        self.script
            .as_ref()
            .is_some_and(|path| !path.as_os_str().is_empty())
            || self
                .inline_script
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }
}

#[derive(Debug)]
struct GtsHookRequest {
    script: GtsHookScript,
    file: PathBuf,
    execute_args: serde_json::Value,
    timeout: Duration,
    instruction_limit: u64,
}

#[derive(Debug)]
enum GtsHookScript {
    Source(String),
    File(PathBuf),
}

#[derive(Debug)]
struct GtsHookJob {
    request: GtsHookRequest,
    deadline: Instant,
    respond_to: oneshot::Sender<Vec<HookOutput>>,
}

#[derive(Debug)]
struct GtsHookWorker {
    tx: mpsc::Sender<GtsHookJob>,
}

impl GtsHookWorker {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel::<GtsHookJob>();
        std::thread::Builder::new()
            .name("night24-gts-hook-worker".to_string())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    let outputs = if Instant::now() >= job.deadline {
                        vec![HookOutput {
                            source: String::new(),
                            stream: OutputStream::Stderr,
                            text: format!(
                                "hook timed out after {} ms",
                                job.request.timeout.as_millis()
                            ),
                        }]
                    } else {
                        run_gts_request(job.request)
                    };
                    let _ = job.respond_to.send(outputs);
                }
            })
            .expect("failed to spawn gts hook worker");
        Self { tx }
    }

    async fn run(&self, request: GtsHookRequest) -> Result<Vec<HookOutput>, String> {
        let timeout = request.timeout;
        let (respond_to, response) = oneshot::channel();
        self.tx
            .send(GtsHookJob {
                request,
                deadline: Instant::now() + timeout,
                respond_to,
            })
            .map_err(|_| "gts hook worker is not available".to_string())?;

        match tokio::time::timeout(timeout, response).await {
            Ok(Ok(outputs)) => Ok(outputs),
            Ok(Err(_)) => Err("gts hook worker stopped before returning output".to_string()),
            Err(_) => Err(format!("hook timed out after {} ms", timeout.as_millis())),
        }
    }
}

impl GtsHookRequest {
    fn from_hook(
        hook: &HookDefinition,
        context: &HookContext<'_>,
        timeout: Duration,
    ) -> anyhow::Result<Self> {
        let context_json: serde_json::Value = serde_json::from_str(&hook_context_json(context))?;
        let file = hook
            .script
            .as_ref()
            .map(|script| {
                if script.is_absolute() {
                    script.clone()
                } else {
                    context.working_dir.join(script)
                }
            })
            .unwrap_or_else(|| context.working_dir.join("<night24-hook.gs>"));
        let script = if let Some(inline_script) = hook
            .inline_script
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            GtsHookScript::Source(inline_script.to_string())
        } else if hook
            .script
            .as_ref()
            .is_some_and(|path| !path.as_os_str().is_empty())
        {
            GtsHookScript::File(file.clone())
        } else {
            anyhow::bail!("gts hook is missing script or inline_script");
        };
        let execute_args = gts_execute_args(hook, context, &context_json, &file);
        Ok(Self {
            script,
            file,
            execute_args,
            timeout,
            instruction_limit: hook.instruction_limit.unwrap_or(1_000_000),
        })
    }
}

fn gts_execute_args(
    hook: &HookDefinition,
    context: &HookContext<'_>,
    context_json: &serde_json::Value,
    file: &Path,
) -> serde_json::Value {
    let hook_name = hook.name.as_deref();
    let script = hook
        .script
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    serde_json::json!({
        "event": context.event.as_str(),
        "run_id": context.run_id,
        "working_dir": context.working_dir.to_string_lossy().to_string(),
        "provider": context.provider,
        "model": context.model,
        "message_count": context.message_count,
        "tool_count": context.tool_count,
        "tool_call_id": context.tool_call_id,
        "tool_name": context.tool_name,
        "summary": context.summary,
        "arguments": context.arguments,
        "result_preview": context.result_preview,
        "error": context.error,
        "duration_ms": context.duration_ms,
        "finish_status": context.finish_status,
        "context": context_json,
        "hook": {
            "event": hook.event.as_str(),
            "name": hook_name,
            "engine": hook.engine.unwrap_or(HookEngine::Gts).to_string(),
            "script": script,
            "file": file.to_string_lossy().to_string(),
            "inline_script": hook.inline_script.as_ref().is_some_and(|value| !value.trim().is_empty()),
            "timeout_ms": hook.timeout_ms,
            "instruction_limit": hook.instruction_limit,
        }
    })
}

fn run_gts_request(request: GtsHookRequest) -> Vec<HookOutput> {
    let session = Session::new();
    let legacy_context = request
        .execute_args
        .get("context")
        .unwrap_or(&request.execute_args);
    session.set_global_json("night24", legacy_context);
    session
        .vm()
        .set_instruction_limit(request.instruction_limit);

    let result = match request.script {
        GtsHookScript::Source(source) => {
            session.vm().set_timeout(Some(request.timeout));
            let result = session.run_source_with_options(&source, &request.file, false);
            session.vm().clear_timeout();
            result
        }
        GtsHookScript::File(path) => session.run_file_with_options(
            &path,
            RunOptions {
                argv: vec![path.to_string_lossy().to_string()],
                call_main: false,
                timeout: Some(request.timeout),
            },
        ),
    };

    let mut outputs = take_gts_vm_output(&session);
    if let Err(err) = result {
        push_output(&mut outputs, "", OutputStream::Stderr, err.inspect());
        return outputs;
    }

    session.vm().set_timeout(Some(request.timeout));
    let execute_result = session.call_execute_json(&request.execute_args);
    session.vm().clear_timeout();
    outputs.extend(take_gts_vm_output(&session));
    match execute_result {
        Ok(Some(value)) => push_structured_gts_outputs(&mut outputs, &value),
        Ok(None) => {}
        Err(err) => push_output(&mut outputs, "", OutputStream::Stderr, err.inspect()),
    }
    outputs
}

fn take_gts_vm_output(session: &Session) -> Vec<HookOutput> {
    let mut outputs = Vec::new();
    let vm_output = session.vm().take_output();
    for text in vm_output.stdout {
        push_output(&mut outputs, "", OutputStream::Stdout, text);
    }
    for text in vm_output.stderr {
        push_output(&mut outputs, "", OutputStream::Stderr, text);
    }
    outputs
}

fn push_structured_gts_outputs(outputs: &mut Vec<HookOutput>, value: &serde_json::Value) {
    let Some(items) = value.get("outputs").and_then(|outputs| outputs.as_array()) else {
        return;
    };
    for item in items {
        let stream = match item.get("stream").and_then(|stream| stream.as_str()) {
            Some("stderr") => OutputStream::Stderr,
            Some("stdout") | None => OutputStream::Stdout,
            Some(_) => OutputStream::Stdout,
        };
        let text = match item.get("text") {
            Some(serde_json::Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
            None => String::new(),
        };
        push_output(outputs, "", stream, text);
    }
}

fn push_output(outputs: &mut Vec<HookOutput>, source: &str, stream: OutputStream, text: String) {
    let text = trim_output(&text);
    if !text.is_empty() {
        outputs.push(HookOutput {
            source: source.to_string(),
            stream,
            text,
        });
    }
}

fn trim_output(text: &str) -> String {
    let trimmed = text.trim().to_string();
    if trimmed.chars().count() <= MAX_OUTPUT_CHARS {
        trimmed
    } else {
        trimmed.chars().take(MAX_OUTPUT_CHARS).collect::<String>() + "..."
    }
}
