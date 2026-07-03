use super::*;
use crate::hooks::{hook_context_json, HookContext, HookEvent, HookRunner};
use serde_json::json;

#[tokio::test]
async fn ping_works_before_initialize() {
    let mut core = AgentCore::default();
    let output = core
        .handle_line(
            r#"{"jsonrpc":"2.0","id":"rpc-1","method":"core.ping","params":{"nonce":"abc"}}"#,
        )
        .await;

    assert_eq!(output.len(), 1);
    let value: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
    assert_eq!(value["result"]["nonce"], "abc");
    assert_eq!(value["result"]["status"], "ok");
}

#[tokio::test]
async fn tools_require_initialize() {
    let mut core = AgentCore::default();
    let output = core
        .handle_line(r#"{"jsonrpc":"2.0","id":"rpc-1","method":"agent.tools","params":{}}"#)
        .await;

    let value: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
    assert_eq!(
        value["error"]["code"],
        night24_protocol::CORE_NOT_INITIALIZED
    );
}

#[tokio::test]
async fn initialize_then_tools_returns_builtin_tools() {
    let mut core = initialized_core().await;
    let output = core
            .handle_line(r#"{"jsonrpc":"2.0","id":"rpc-tools","method":"agent.tools","params":{"include_disabled":false}}"#)
            .await;

    let value: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
    let tools = value["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|tool| tool["name"] == "developer__echo"));
}

#[tokio::test]
async fn reply_returns_accepted_message_and_finish() {
    let mut core = initialized_core().await;
    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-1",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": ".",
                "conversation": []
            },
            "input": { "text": "hello" },
            "provider": { "provider": "echo", "model": "echo-v1" },
            "limits": {
                "max_turns": 1,
                "turn_timeout_ms": 10000,
                "tool_timeout_ms": 10000,
                "total_timeout_ms": 30000
            },
            "options": {
                "stream_message_delta": false,
                "emit_tool_events": true,
                "permission_mode": "permissive"
            }
        }
    });

    let output = core.handle_line(&request.to_string()).await;

    assert!(output.len() >= 3);
    let accepted: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
    assert_eq!(accepted["result"]["accepted"], true);
    assert_eq!(accepted["result"]["run_id"], "run-1");

    let message: serde_json::Value = serde_json::from_str(&output[1]).unwrap();
    assert_eq!(message["method"], "agent.event");
    assert_eq!(message["params"]["type"], "message");
    assert_eq!(message["params"]["payload"]["message"]["role"], "assistant");

    let finish: serde_json::Value = serde_json::from_str(output.last().unwrap()).unwrap();
    assert_eq!(finish["params"]["type"], "finish");
    assert_eq!(finish["params"]["payload"]["status"], "completed");
}

#[tokio::test]
async fn strict_tool_call_waits_for_permission_and_continues_after_approve() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut core = AgentCore::with_output(tx);
    initialize_core(&mut core).await;

    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-permission",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": ".",
                "conversation": []
            },
            "input": { "text": "tool:datetime:" },
            "provider": { "provider": "echo", "model": "echo-v1" },
            "limits": {
                "max_turns": 1,
                "turn_timeout_ms": 10000,
                "tool_timeout_ms": 10000,
                "total_timeout_ms": 30000
            },
            "options": {
                "stream_message_delta": false,
                "emit_tool_events": true,
                "permission_mode": "strict"
            }
        }
    });

    let accepted = core.handle_line(&request.to_string()).await;
    let accepted: serde_json::Value = serde_json::from_str(&accepted[0]).unwrap();
    assert_eq!(accepted["result"]["accepted"], true);

    let permission = next_event_of_type(&mut rx, "permission_required").await;
    let permission_id = permission["params"]["payload"]["permission_id"]
        .as_str()
        .unwrap()
        .to_string();

    let resolve = json!({
        "jsonrpc": "2.0",
        "id": "rpc-permission",
        "method": "permission.resolve",
        "params": {
            "run_id": "run-permission",
            "permission_id": permission_id,
            "decision": "approve"
        }
    });
    let resolved = core.handle_line(&resolve.to_string()).await;
    let resolved: serde_json::Value = serde_json::from_str(&resolved[0]).unwrap();
    assert_eq!(resolved["result"]["accepted"], true);

    let tool_started = next_event_of_type(&mut rx, "tool_started").await;
    assert_eq!(
        tool_started["params"]["payload"]["tool_name"],
        "developer__datetime"
    );
    let finish = next_event_of_type(&mut rx, "finish").await;
    assert_eq!(finish["params"]["payload"]["status"], "completed");
}

#[tokio::test]
async fn cancel_unblocks_pending_permission_and_finishes_cancelled() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut core = AgentCore::with_output(tx);
    initialize_core(&mut core).await;

    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-cancel",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": ".",
                "conversation": []
            },
            "input": { "text": "tool:datetime:" },
            "provider": { "provider": "echo", "model": "echo-v1" },
            "limits": {
                "max_turns": 1,
                "turn_timeout_ms": 10000,
                "tool_timeout_ms": 10000,
                "total_timeout_ms": 30000
            },
            "options": {
                "stream_message_delta": false,
                "emit_tool_events": true,
                "permission_mode": "strict"
            }
        }
    });

    let accepted = core.handle_line(&request.to_string()).await;
    let accepted: serde_json::Value = serde_json::from_str(&accepted[0]).unwrap();
    assert_eq!(accepted["result"]["accepted"], true);
    let _permission = next_event_of_type(&mut rx, "permission_required").await;

    let cancel = json!({
        "jsonrpc": "2.0",
        "id": "rpc-cancel",
        "method": "agent.cancel",
        "params": {
            "run_id": "run-cancel",
            "reason": "test"
        }
    });
    let cancelled = core.handle_line(&cancel.to_string()).await;
    let cancelled: serde_json::Value = serde_json::from_str(&cancelled[0]).unwrap();
    assert_eq!(cancelled["result"]["accepted"], true);

    let finish = next_event_of_type(&mut rx, "finish").await;
    assert_eq!(finish["params"]["payload"]["status"], "cancelled");
}

#[tokio::test]
async fn full_access_returns_sensitive_tool_output_without_prompting() {
    let temp_dir = test_temp_dir("full-access-sensitive").await;
    tokio::fs::write(
        temp_dir.join("secret.txt"),
        "OPENAI_API_KEY=sk-test1234567890abcdef",
    )
    .await
    .unwrap();

    let mut core = initialized_core().await;
    let working_dir = temp_dir.to_string_lossy().to_string();
    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-full-access-sensitive",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": working_dir,
                "conversation": []
            },
            "input": { "text": "tool:read:secret.txt" },
            "provider": { "provider": "echo", "model": "echo-v1" },
            "limits": {
                "max_turns": 2,
                "turn_timeout_ms": 10000,
                "tool_timeout_ms": 10000,
                "total_timeout_ms": 30000
            },
            "options": {
                "stream_message_delta": false,
                "emit_tool_events": true,
                "permission_mode": "allow_all"
            }
        }
    });

    let output = core.handle_line(&request.to_string()).await;
    let joined = output.join("\n");

    assert!(joined.contains("sk-test1234567890abcdef"));
    assert!(!joined.contains("developer__sensitive_output"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn sensitive_tool_output_requires_user_decision_outside_full_access() {
    let temp_dir = test_temp_dir("confirm-sensitive").await;
    tokio::fs::write(
        temp_dir.join("secret.txt"),
        "Project notes\nOPENAI_API_KEY=sk-test1234567890abcdef",
    )
    .await
    .unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut core = AgentCore::with_output(tx);
    initialize_core(&mut core).await;
    let working_dir = temp_dir.to_string_lossy().to_string();

    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-sensitive-confirm",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": working_dir,
                "conversation": []
            },
            "input": { "text": "tool:read:secret.txt" },
            "provider": { "provider": "echo", "model": "echo-v1" },
            "limits": {
                "max_turns": 2,
                "turn_timeout_ms": 10000,
                "tool_timeout_ms": 10000,
                "total_timeout_ms": 30000
            },
            "options": {
                "stream_message_delta": false,
                "emit_tool_events": true,
                "permission_mode": "permissive"
            }
        }
    });

    let accepted = core.handle_line(&request.to_string()).await;
    let accepted: serde_json::Value = serde_json::from_str(&accepted[0]).unwrap();
    assert_eq!(accepted["result"]["accepted"], true);

    let permission = next_event_of_type(&mut rx, "permission_required").await;
    assert_eq!(
        permission["params"]["payload"]["tool_name"],
        "developer__sensitive_output"
    );
    let permission_text = permission.to_string();
    assert!(permission_text.contains("[redacted sensitive value]"));
    assert!(!permission_text.contains("sk-test1234567890abcdef"));

    let permission_id = permission["params"]["payload"]["permission_id"]
        .as_str()
        .unwrap()
        .to_string();
    let resolve = json!({
        "jsonrpc": "2.0",
        "id": "rpc-permission",
        "method": "permission.resolve",
        "params": {
            "run_id": "run-sensitive-confirm",
            "permission_id": permission_id,
            "decision": "deny"
        }
    });
    let resolved = core.handle_line(&resolve.to_string()).await;
    let resolved: serde_json::Value = serde_json::from_str(&resolved[0]).unwrap();
    assert_eq!(resolved["result"]["accepted"], true);

    let finish = next_event_of_type(&mut rx, "finish").await;
    assert_eq!(finish["params"]["payload"]["status"], "completed");
    let finish_text = finish.to_string();
    assert!(finish_text.contains("[redacted sensitive value]"));
    assert!(!finish_text.contains("sk-test1234567890abcdef"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[test]
fn stepfun_provider_requires_key_without_falling_back_to_echo() {
    let config = ProviderConfig {
        provider: "stepfun".to_string(),
        model: "step-3.7-flash".to_string(),
        base_url: Some("https://api.stepfun.com/step_plan/v1".to_string()),
        api_key_ref: None,
        api_key: None,
    };

    let error = match create_provider(&config) {
        Ok(_) => panic!("stepfun provider should require an API key"),
        Err(err) => err.to_string(),
    };
    assert!(error.contains("api_key is required for stepfun provider"));
}

#[test]
fn stepfun_provider_uses_inline_request_config() {
    let config = ProviderConfig {
        provider: "stepfun".to_string(),
        model: "step-3.7-flash".to_string(),
        base_url: Some("https://api.stepfun.com/step_plan/v1".to_string()),
        api_key_ref: None,
        api_key: Some("test-key".to_string()),
    };

    let provider = create_provider(&config).unwrap();
    assert_eq!(provider.name(), "openai");
}

#[test]
fn network_proxy_is_injected_only_for_network_tools() {
    let args = json!({"url": "https://example.com"});
    let with_proxy = arguments_with_network_proxy(
        "developer__web_search",
        &args,
        Some("http://127.0.0.1:7890"),
    );
    assert_eq!(with_proxy["proxy"], "http://127.0.0.1:7890");

    let non_network =
        arguments_with_network_proxy("developer__read_file", &args, Some("http://127.0.0.1:7890"));
    assert!(non_network.get("proxy").is_none());
}

#[test]
fn network_proxy_does_not_override_tool_argument() {
    let args = json!({"url": "https://example.com", "proxy": "direct"});
    let with_proxy = arguments_with_network_proxy(
        "developer__http_request",
        &args,
        Some("http://127.0.0.1:7890"),
    );
    assert_eq!(with_proxy["proxy"], "direct");
}

#[test]
fn hook_config_ignores_command_only_hooks() {
    let runner = HookRunner::from_config_str(
        r#"{
            "hooks": [
                { "event": "run_started", "command": "echo start" },
                { "event": "before_tool", "command": "" },
                { "event": "run_finished", "script": "hooks/done.gs", "enabled": false }
            ]
        }"#,
    )
    .unwrap();

    assert!(runner.is_empty());
}

#[test]
fn hook_config_accepts_gts_script_hooks() {
    let runner = HookRunner::from_config_str(
        r#"{
            "hooks": [
                {
                    "event": "before_provider_request",
                    "name": "provider-policy",
                    "engine": "gts",
                    "script": "hooks/provider_policy.gs",
                    "timeout_ms": 5000
                }
            ]
        }"#,
    )
    .unwrap();

    assert!(!runner.is_empty());
}

#[tokio::test]
async fn gts_hook_calls_execute_with_args_and_structured_outputs() {
    let temp_dir = test_temp_dir("gts-execute-hook").await;
    let hook_dir = temp_dir.join("hooks");
    tokio::fs::create_dir_all(&hook_dir).await.unwrap();
    tokio::fs::write(
        hook_dir.join("audit.gs"),
        r#"function execute(args) {
  return {
    outputs: [
      {
        stream: "stdout",
        text: "event=" + args.event + " run=" + args.run_id + " tool=" + args.tool_name + " text=" + args.arguments.text
      },
      {
        stream: "stderr",
        text: "summary=" + args.summary + " cwd=" + args.working_dir
      }
    ]
  };
}
"#,
    )
    .await
    .unwrap();

    let config = serde_json::json!({
        "hooks": [
            {
                "event": "before_tool",
                "name": "tool-audit",
                "engine": "gts",
                "script": "hooks/audit.gs",
                "timeout_ms": 5000
            }
        ]
    });
    let runner = HookRunner::from_config_str(&config.to_string()).unwrap();

    let outputs = runner
        .run(&HookContext {
            event: HookEvent::BeforeTool,
            run_id: "run-gts-execute",
            working_dir: &temp_dir,
            provider: None,
            model: None,
            message_count: None,
            tool_count: None,
            tool_call_id: Some("tool-1"),
            tool_name: Some("developer__echo"),
            summary: Some("Call developer__echo"),
            arguments: Some(&json!({"text": "hello"})),
            result_preview: None,
            error: None,
            duration_ms: None,
            finish_status: None,
        })
        .await;

    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].source, "hook:before_tool:tool-audit");
    assert!(matches!(
        outputs[0].stream,
        night24_protocol::OutputStream::Stdout
    ));
    assert_eq!(
        outputs[0].text,
        "event=before_tool run=run-gts-execute tool=developer__echo text=hello"
    );
    assert_eq!(outputs[1].source, "hook:before_tool:tool-audit");
    assert!(matches!(
        outputs[1].stream,
        night24_protocol::OutputStream::Stderr
    ));
    assert!(outputs[1]
        .text
        .contains("summary=Call developer__echo cwd="));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn gts_hook_can_call_cli_from_inside_script() {
    let temp_dir = test_temp_dir("gts-cli-hook").await;
    let hook_dir = temp_dir.join("hooks");
    tokio::fs::create_dir_all(&hook_dir).await.unwrap();
    tokio::fs::write(
        hook_dir.join("cli.gs"),
        r#"let exec = require("@std/exec");
let os = require("@std/os");

function execute(args) {
  let output = "";
  if (os.platform === "windows") {
    output = exec.output("cmd", ["/C", "echo cli-ok"]);
  } else {
    output = exec.output("sh", ["-c", "printf cli-ok"]);
  }
  return {
    outputs: [
      {
        stream: "stdout",
        text: "cli=" + output
      }
    ]
  };
}
"#,
    )
    .await
    .unwrap();

    let config = serde_json::json!({
        "hooks": [
            {
                "event": "run_started",
                "name": "cli-hook",
                "engine": "gts",
                "script": "hooks/cli.gs",
                "timeout_ms": 5000
            }
        ]
    });
    let runner = HookRunner::from_config_str(&config.to_string()).unwrap();

    let outputs = runner
        .run(&HookContext {
            event: HookEvent::RunStarted,
            run_id: "run-gts-cli",
            working_dir: &temp_dir,
            provider: None,
            model: None,
            message_count: None,
            tool_count: None,
            tool_call_id: None,
            tool_name: None,
            summary: None,
            arguments: None,
            result_preview: None,
            error: None,
            duration_ms: None,
            finish_status: None,
        })
        .await;

    let stdout = outputs
        .iter()
        .filter(|output| matches!(output.stream, night24_protocol::OutputStream::Stdout))
        .map(|output| output.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(stdout.contains("cli=cli-ok"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[test]
fn hook_context_json_includes_provider_and_tool_fields() {
    let temp_dir = std::path::PathBuf::from("E:/workspace/example");
    let args = json!({"command": "date"});
    let json_text = hook_context_json(&HookContext {
        event: HookEvent::BeforeTool,
        run_id: "run-json",
        working_dir: &temp_dir,
        provider: Some("openai"),
        model: Some("gpt-4o-mini"),
        message_count: Some(4),
        tool_count: Some(18),
        tool_call_id: Some("tool-1"),
        tool_name: Some("developer__shell"),
        summary: Some("Run shell command: date"),
        arguments: Some(&args),
        result_preview: None,
        error: None,
        duration_ms: None,
        finish_status: None,
    });
    let value: serde_json::Value = serde_json::from_str(&json_text).unwrap();

    assert_eq!(value["event"], "before_tool");
    assert_eq!(value["run_id"], "run-json");
    assert_eq!(value["provider"], "openai");
    assert_eq!(value["model"], "gpt-4o-mini");
    assert_eq!(value["message_count"], 4);
    assert_eq!(value["tool_count"], 18);
    assert_eq!(value["tool_name"], "developer__shell");
    assert_eq!(value["arguments"]["command"], "date");
}

async fn initialized_core() -> AgentCore {
    let mut core = AgentCore::default();
    initialize_core(&mut core).await;
    core
}

async fn initialize_core(core: &mut AgentCore) {
    let output = core
            .handle_line(
                r#"{"jsonrpc":"2.0","id":"rpc-init","method":"core.initialize","params":{"protocol_version":"2026-07-01","client":{"name":"night24-server","version":"0.1.0"},"capabilities":[]}}"#,
            )
            .await;
    let value: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
    assert_eq!(value["result"]["protocol_version"], "2026-07-01");
}

async fn test_temp_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("night24-{name}-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&path).await.unwrap();
    path
}

async fn next_event_of_type(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    event_type: &str,
) -> serde_json::Value {
    for _ in 0..20 {
        let raw = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for agent event")
            .expect("agent event channel closed");
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        if value["params"]["type"] == event_type {
            return value;
        }
    }
    panic!("event type {event_type} was not emitted");
}
