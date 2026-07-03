use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use night24_core::model::{ContentBlock, Message};
use night24_core::permission::{PermissionLevel, PermissionManager, ToolCategory};
use night24_core::security::SecurityInspector;
use night24_core::tool_executor::execute_tool_raw_output;
use night24_protocol::{AgentEventKind, PermissionDecision, RiskLevel};
use tokio::sync::oneshot;
use tokio::time::Instant;

use crate::hooks::{HookContext, HookEvent};
use crate::types::{PermissionHandle, RunContext, RunHandle};

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

    match security.require_permission(tool_name).await {
        PermissionLevel::Deny => anyhow::bail!("permission denied for {tool_name}"),
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
                    summary: Some(&summary),
                    arguments: Some(&arguments),
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
                summary: summary.clone(),
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
        }
        PermissionLevel::Allow => {}
    }

    if context.is_cancelled() {
        anyhow::bail!("cancelled");
    }

    context
        .run_hooks(HookContext {
            event: HookEvent::BeforeTool,
            run_id: &context.run_id,
            working_dir,
            provider: None,
            model: None,
            message_count: None,
            tool_count: None,
            tool_call_id: Some(tool_call_id),
            tool_name: Some(tool_name),
            summary: Some(&summary),
            arguments: Some(&arguments),
            result_preview: None,
            error: None,
            duration_ms: None,
            finish_status: None,
        })
        .await;

    if context.emit_tool_events {
        context.send(AgentEventKind::ToolStarted {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            summary,
            arguments: arguments.clone(),
        });
    }

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
            context
                .run_hooks(HookContext {
                    event: HookEvent::AfterTool,
                    run_id: &context.run_id,
                    working_dir,
                    provider: None,
                    model: None,
                    message_count: None,
                    tool_count: None,
                    tool_call_id: Some(tool_call_id),
                    tool_name: Some(tool_name),
                    summary: None,
                    arguments: Some(&arguments),
                    result_preview: None,
                    error: Some(&error),
                    duration_ms: Some(duration_ms),
                    finish_status: None,
                })
                .await;
            return Err(err);
        }
    };

    if context.is_cancelled() {
        anyhow::bail!("cancelled");
    }

    let result_preview = preview(&result);
    context
        .run_hooks(HookContext {
            event: HookEvent::AfterTool,
            run_id: &context.run_id,
            working_dir,
            provider: None,
            model: None,
            message_count: None,
            tool_count: None,
            tool_call_id: Some(tool_call_id),
            tool_name: Some(tool_name),
            summary: None,
            arguments: Some(&arguments),
            result_preview: Some(&result_preview),
            error: None,
            duration_ms: Some(duration_ms),
            finish_status: None,
        })
        .await;

    if context.emit_tool_events {
        context.send(AgentEventKind::ToolFinished {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            duration_ms,
            summary: format!("{} completed", tool_name),
            result_preview,
            is_error: false,
        });
    }

    Ok(result)
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
    match mode
        .unwrap_or("strict")
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .as_str()
    {
        "allow_all" | "full_access" => PermissionManager::new(PermissionLevel::Allow),
        "deny_all" => PermissionManager::new(PermissionLevel::Deny),
        "permissive" => PermissionManager::permissive_local(),
        _ => PermissionManager::default(),
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

fn risk_for_tool(tool_name: &str) -> RiskLevel {
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
