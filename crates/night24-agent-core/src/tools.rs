use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use night24_core::model::{ContentBlock, Message};
use night24_core::permission::{PermissionLevel, PermissionManager, ToolCategory};
use night24_core::security::SecurityInspector;
use night24_core::tool_executor::execute_tool_raw_output;
use night24_protocol::{AgentEventKind, EventError, PermissionDecision, PermissionMode, RiskLevel};
use tokio::sync::oneshot;
use tokio::time::Instant;

use crate::hooks::{HookContext, HookEvent};
use crate::types::{PermissionHandle, RunContext, RunHandle};

pub(super) struct ToolLifecycle<'a> {
    pub(super) context: &'a RunContext,
    pub(super) working_dir: &'a Path,
    pub(super) tool_call_id: &'a str,
    pub(super) tool_name: &'a str,
    pub(super) arguments: &'a serde_json::Value,
}

impl<'a> ToolLifecycle<'a> {
    pub(super) fn new(
        context: &'a RunContext,
        working_dir: &'a Path,
        tool_call_id: &'a str,
        tool_name: &'a str,
        arguments: &'a serde_json::Value,
    ) -> Self {
        Self {
            context,
            working_dir,
            tool_call_id,
            tool_name,
            arguments,
        }
    }

    pub(super) async fn before(&self, summary: &str) {
        self.context
            .run_hooks(HookContext {
                event: HookEvent::BeforeTool,
                run_id: &self.context.run_id,
                working_dir: self.working_dir,
                provider: None,
                model: None,
                message_count: None,
                tool_count: None,
                tool_call_id: Some(self.tool_call_id),
                tool_name: Some(self.tool_name),
                summary: Some(summary),
                arguments: Some(self.arguments),
                result_preview: None,
                error: None,
                duration_ms: None,
                finish_status: None,
            })
            .await;

        if self.context.emit_tool_events {
            self.context.send(AgentEventKind::ToolStarted {
                tool_call_id: self.tool_call_id.to_string(),
                tool_name: self.tool_name.to_string(),
                summary: summary.to_string(),
                arguments: self.arguments.clone(),
            });
        }
    }

    pub(super) async fn success(&self, duration_ms: u64, summary: String, result_preview: String) {
        self.context
            .run_hooks(HookContext {
                event: HookEvent::AfterTool,
                run_id: &self.context.run_id,
                working_dir: self.working_dir,
                provider: None,
                model: None,
                message_count: None,
                tool_count: None,
                tool_call_id: Some(self.tool_call_id),
                tool_name: Some(self.tool_name),
                summary: None,
                arguments: Some(self.arguments),
                result_preview: Some(&result_preview),
                error: None,
                duration_ms: Some(duration_ms),
                finish_status: None,
            })
            .await;

        if self.context.emit_tool_events {
            self.context.send(AgentEventKind::ToolFinished {
                tool_call_id: self.tool_call_id.to_string(),
                tool_name: self.tool_name.to_string(),
                duration_ms,
                summary,
                result_preview,
                is_error: false,
            });
        }
    }

    pub(super) async fn failure(&self, duration_ms: u64, code: &str, error: &str) {
        self.context
            .run_hooks(HookContext {
                event: HookEvent::AfterTool,
                run_id: &self.context.run_id,
                working_dir: self.working_dir,
                provider: None,
                model: None,
                message_count: None,
                tool_count: None,
                tool_call_id: Some(self.tool_call_id),
                tool_name: Some(self.tool_name),
                summary: None,
                arguments: Some(self.arguments),
                result_preview: None,
                error: Some(error),
                duration_ms: Some(duration_ms),
                finish_status: None,
            })
            .await;

        if self.context.emit_tool_events {
            self.context.send(AgentEventKind::ToolFailed {
                tool_call_id: self.tool_call_id.to_string(),
                tool_name: self.tool_name.to_string(),
                duration_ms,
                error: EventError {
                    code: code.to_string(),
                    message: error.to_string(),
                },
            });
        }
    }
}

pub(super) async fn execute_tool_with_events(
    context: &RunContext,
    security: &SecurityInspector,
    tool_call_id: &str,
    tool_name: &str,
    arguments: &serde_json::Value,
    working_dir: &std::path::Path,
    tool_timeout: Duration,
    network_proxy: Option<&str>,
) -> anyhow::Result<String> {
    if context.is_cancelled() {
        anyhow::bail!("cancelled");
    }

    let arguments = arguments_with_network_proxy(tool_name, arguments, network_proxy);
    let summary = summarize_tool_call(tool_name, &arguments);

    ensure_tool_permission(
        context,
        security,
        working_dir,
        tool_call_id,
        tool_name,
        &arguments,
        &summary,
    )
    .await?;

    if context.is_cancelled() {
        anyhow::bail!("cancelled");
    }

    let lifecycle = ToolLifecycle::new(context, working_dir, tool_call_id, tool_name, &arguments);
    lifecycle.before(&summary).await;

    let started = Instant::now();
    let result = match tokio::time::timeout(
        tool_timeout,
        execute_tool_raw_output(tool_name, &arguments, working_dir, security),
    )
    .await
    .map_err(|_| anyhow::anyhow!("tool timed out after {:?}", tool_timeout))?
    {
        Ok(result) => {
            resolve_sensitive_output(
                context,
                security,
                tool_call_id,
                tool_name,
                &arguments,
                result,
            )
            .await
        }
        Err(err) => Err(err),
    };
    let duration_ms = started.elapsed().as_millis() as u64;

    let result = match result {
        Ok(result) => result,
        Err(err) => {
            let error = err.to_string();
            lifecycle.failure(duration_ms, "tool_failed", &error).await;
            return Err(err);
        }
    };

    if context.is_cancelled() {
        anyhow::bail!("cancelled");
    }

    let result_preview = preview(&result);
    lifecycle
        .success(
            duration_ms,
            format!("{} completed", tool_name),
            result_preview,
        )
        .await;

    Ok(result)
}

pub(super) async fn ensure_tool_permission(
    context: &RunContext,
    security: &SecurityInspector,
    working_dir: &Path,
    tool_call_id: &str,
    tool_name: &str,
    arguments: &serde_json::Value,
    summary: &str,
) -> anyhow::Result<()> {
    match security.require_permission(tool_name).await {
        PermissionLevel::Deny => anyhow::bail!("permission denied for {tool_name}"),
        PermissionLevel::Allow => Ok(()),
        PermissionLevel::Confirm => {
            let permission_id = format!("perm-{}", uuid::Uuid::new_v4());
            let (tx, rx) = oneshot::channel();
            context
                .permissions
                .lock()
                .map_err(|_| anyhow::anyhow!("permission state lock poisoned"))?
                .insert(
                    permission_id.clone(),
                    PermissionHandle {
                        run_id: context.run_id.clone(),
                        sender: tx,
                    },
                );

            context
                .run_hooks(HookContext {
                    event: HookEvent::PermissionRequired,
                    run_id: &context.run_id,
                    working_dir,
                    provider: None,
                    model: None,
                    message_count: None,
                    tool_count: None,
                    tool_call_id: Some(tool_call_id),
                    tool_name: Some(tool_name),
                    summary: Some(summary),
                    arguments: Some(arguments),
                    result_preview: None,
                    error: None,
                    duration_ms: None,
                    finish_status: None,
                })
                .await;
            context.send(AgentEventKind::PermissionRequired {
                permission_id,
                tool_call_id: tool_call_id.to_string(),
                tool_name: tool_name.to_string(),
                risk: risk_for_tool(tool_name),
                summary: summary.to_string(),
                arguments: arguments.clone(),
                timeout_ms: 300_000,
            });

            let decision = tokio::time::timeout(Duration::from_secs(300), rx)
                .await
                .map_err(|_| anyhow::anyhow!("permission request timed out"))?
                .map_err(|_| anyhow::anyhow!("permission request closed"))?;

            if matches!(decision, PermissionDecision::Deny) {
                anyhow::bail!("permission denied for {tool_name}");
            }
            Ok(())
        }
    }
}

async fn resolve_sensitive_output(
    context: &RunContext,
    security: &SecurityInspector,
    tool_call_id: &str,
    tool_name: &str,
    arguments: &serde_json::Value,
    result: String,
) -> anyhow::Result<String> {
    let inspection = security.sanitize_output(&result).await;
    if inspection.findings.is_empty() {
        return Ok(result);
    }

    match security
        .require_permission("developer__sensitive_output")
        .await
    {
        PermissionLevel::Allow => Ok(result),
        PermissionLevel::Deny => Ok(format!(
            "security inspection: {}\n\n{}",
            inspection.findings.join("; "),
            inspection.sanitized
        )),
        PermissionLevel::Confirm => {
            let permission_id = format!("perm-{}", uuid::Uuid::new_v4());
            let (tx, rx) = oneshot::channel();
            context
                .permissions
                .lock()
                .map_err(|_| anyhow::anyhow!("permission state lock poisoned"))?
                .insert(
                    permission_id.clone(),
                    PermissionHandle {
                        run_id: context.run_id.clone(),
                        sender: tx,
                    },
                );

            context.send(AgentEventKind::PermissionRequired {
                permission_id,
                tool_call_id: format!("{tool_call_id}:sensitive_output"),
                tool_name: "developer__sensitive_output".to_string(),
                risk: RiskLevel::High,
                summary: "工具输出可能包含敏感凭证。批准返回原文，拒绝返回屏蔽版。".to_string(),
                arguments: serde_json::json!({
                    "source_tool": tool_name,
                    "source_arguments": arguments,
                    "findings": inspection.findings.clone(),
                    "redacted_preview": preview(&inspection.sanitized),
                }),
                timeout_ms: 300_000,
            });

            let decision = tokio::time::timeout(Duration::from_secs(300), rx)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or(PermissionDecision::Deny);

            if matches!(decision, PermissionDecision::Approve) {
                Ok(result)
            } else {
                Ok(format!(
                    "security inspection: {}\n\n{}",
                    inspection.findings.join("; "),
                    inspection.sanitized
                ))
            }
        }
    }
}

pub(super) fn permission_manager_for_mode(mode: Option<&str>) -> PermissionManager {
    match PermissionMode::normalize(mode) {
        PermissionMode::AllowAll => PermissionManager::new(PermissionLevel::Allow),
        PermissionMode::DenyAll => PermissionManager::new(PermissionLevel::Deny),
        PermissionMode::Permissive => PermissionManager::permissive_local(),
        PermissionMode::Strict => PermissionManager::default(),
    }
}

pub(super) fn text_content(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } | ContentBlock::Thinking { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn risk_for_tool(tool_name: &str) -> RiskLevel {
    match ToolCategory::from_tool_name(tool_name) {
        ToolCategory::Shell | ToolCategory::Write => RiskLevel::High,
        ToolCategory::Network => RiskLevel::Medium,
        ToolCategory::Read | ToolCategory::Other => RiskLevel::Low,
    }
}

pub(super) fn arguments_with_network_proxy(
    tool_name: &str,
    arguments: &serde_json::Value,
    network_proxy: Option<&str>,
) -> serde_json::Value {
    let Some(proxy) = network_proxy
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return arguments.clone();
    };
    if ToolCategory::from_tool_name(tool_name) != ToolCategory::Network {
        return arguments.clone();
    }
    let mut value = arguments.clone();
    let Some(object) = value.as_object_mut() else {
        return value;
    };
    if object
        .get("proxy")
        .and_then(|value| value.as_str())
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        return value;
    }
    object.insert("proxy".to_string(), serde_json::json!(proxy));
    value
}

fn summarize_tool_call(tool_name: &str, arguments: &serde_json::Value) -> String {
    match tool_name {
        "developer__shell" => arguments
            .get("command")
            .and_then(|value| value.as_str())
            .map(|value| format!("Run shell command: {}", value))
            .unwrap_or_else(|| "Run shell command".to_string()),
        "developer__write_file" => arguments
            .get("path")
            .and_then(|value| value.as_str())
            .map(|value| format!("Write file: {}", value))
            .unwrap_or_else(|| "Write file".to_string()),
        "developer__read_file" => arguments
            .get("path")
            .and_then(|value| value.as_str())
            .map(|value| format!("Read file: {}", value))
            .unwrap_or_else(|| "Read file".to_string()),
        "developer__http_request" | "developer__network_request" => arguments
            .get("url")
            .and_then(|value| value.as_str())
            .map(|value| format!("Request URL: {}", value))
            .unwrap_or_else(|| "Network request".to_string()),
        "developer__web_search" | "developer__network_search" => arguments
            .get("query")
            .and_then(|value| value.as_str())
            .map(|value| format!("Search web: {}", value))
            .unwrap_or_else(|| "Web search".to_string()),
        "developer__web_scraper" => arguments
            .get("url")
            .and_then(|value| value.as_str())
            .map(|value| format!("Scrape URL: {}", value))
            .unwrap_or_else(|| "Scrape web page".to_string()),
        _ => format!("Call {}", tool_name),
    }
}

fn preview(text: &str) -> String {
    const MAX_PREVIEW: usize = 500;
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= MAX_PREVIEW {
        compact
    } else {
        compact.chars().take(MAX_PREVIEW).collect::<String>() + "..."
    }
}

pub(super) fn cleanup_run(
    runs: &Arc<Mutex<HashMap<String, RunHandle>>>,
    permissions: &Arc<Mutex<HashMap<String, PermissionHandle>>>,
    run_id: &str,
) {
    if let Ok(mut runs) = runs.lock() {
        runs.remove(run_id);
    }
    deny_pending_permissions_for_run(permissions, run_id);
}

pub(super) fn deny_pending_permissions_for_run(
    permissions: &Arc<Mutex<HashMap<String, PermissionHandle>>>,
    run_id: &str,
) {
    let pending = permissions
        .lock()
        .ok()
        .map(|mut permissions| {
            let ids = permissions
                .iter()
                .filter_map(|(id, handle)| {
                    if handle.run_id == run_id {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| permissions.remove(&id))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for permission in pending {
        let _ = permission.sender.send(PermissionDecision::Deny);
    }
}
