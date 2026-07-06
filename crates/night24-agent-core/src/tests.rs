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
    assert!(tools
        .iter()
        .any(|tool| tool["name"] == "developer__subagent_spawn"));
    assert!(tools
        .iter()
        .any(|tool| tool["name"] == "developer__skill_load"));
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
async fn run_lifecycle_hooks_include_provider_model_and_counts() {
    let temp_dir = test_temp_dir("run-lifecycle-hook-context").await;
    let hook_dir = temp_dir.join(".night24");
    tokio::fs::create_dir_all(&hook_dir).await.unwrap();
    tokio::fs::write(
        hook_dir.join("hooks.json"),
        r#"{
            "hooks": [
                {
                    "event": "run_started",
                    "name": "started-context",
                    "engine": "gts",
                    "inline_script": "function execute(args) { return { outputs: [{ stream: \"stdout\", text: \"started provider=\" + args.provider + \" model=\" + args.model + \" messages=\" + String(args.message_count) + \" tools=\" + String(args.tool_count) }] }; }"
                },
                {
                    "event": "run_finished",
                    "name": "finished-context",
                    "engine": "gts",
                    "inline_script": "function execute(args) { return { outputs: [{ stream: \"stdout\", text: \"finished provider=\" + args.provider + \" model=\" + args.model + \" messages=\" + String(args.message_count) + \" tools=\" + String(args.tool_count) + \" status=\" + args.finish_status }] }; }"
                }
            ]
        }"#,
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
            "run_id": "run-lifecycle-hook-context",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": working_dir,
                "conversation": [
                    {
                        "id": "previous-user",
                        "role": "user",
                        "content": [{ "type": "text", "text": "previous" }],
                        "created_at": "2026-07-06T00:00:00Z"
                    }
                ]
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
    let hook_texts = output
        .iter()
        .filter_map(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .filter(|value| value["params"]["type"] == "run_output")
        .filter_map(|value| {
            value["params"]["payload"]["text"]
                .as_str()
                .map(str::to_string)
        })
        .collect::<Vec<_>>();

    let started = hook_texts
        .iter()
        .find(|text| text.starts_with("started provider=echo model=echo-v1 messages=2 tools="))
        .unwrap_or_else(|| panic!("missing started lifecycle hook output: {hook_texts:?}"));
    let finished = hook_texts
        .iter()
        .find(|text| {
            text.starts_with("finished provider=echo model=echo-v1 messages=2 tools=")
                && text.ends_with(" status=completed")
        })
        .unwrap_or_else(|| panic!("missing finished lifecycle hook output: {hook_texts:?}"));
    assert_ne!(started.rsplit_once("tools=").unwrap().1, "null");
    assert_ne!(
        finished
            .split("tools=")
            .nth(1)
            .and_then(|value| value.split_whitespace().next())
            .unwrap(),
        "null"
    );

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn agent_skills_returns_workspace_registry() {
    let temp_dir = test_temp_dir("skills-registry").await;
    let skill_dir = temp_dir.join(".night24").join("skills").join("review");
    tokio::fs::create_dir_all(&skill_dir).await.unwrap();
    tokio::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review changed code.\n---\n# Review\nFind bugs.\n",
    )
    .await
    .unwrap();

    let mut core = initialized_core().await;
    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-skills",
        "method": "agent.skills",
        "params": {
            "working_dir": temp_dir
        }
    });
    let output = core.handle_line(&request.to_string()).await;
    let value: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
    assert_eq!(value["result"]["registry"]["skills"][0]["name"], "review");
    assert_eq!(
        value["result"]["registry"]["skills"][0]["description"],
        "Review changed code."
    );

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn agent_skill_load_returns_workspace_skill_body() {
    let temp_dir = test_temp_dir("skill-load-rpc").await;
    let skill_dir = temp_dir.join(".night24").join("skills").join("review");
    tokio::fs::create_dir_all(&skill_dir).await.unwrap();
    tokio::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review changed code.\n---\n# Review\nFind bugs.\n",
    )
    .await
    .unwrap();

    let mut core = initialized_core().await;
    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-skill-load",
        "method": "agent.skill.load",
        "params": {
            "working_dir": temp_dir,
            "name": "review"
        }
    });
    let output = core.handle_line(&request.to_string()).await;
    let value: serde_json::Value = serde_json::from_str(&output[0]).unwrap();
    assert_eq!(value["result"]["skill"]["skill"]["name"], "review");
    assert_eq!(value["result"]["skill"]["body"], "# Review\nFind bugs.\n");

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn skill_load_tool_loads_workspace_skill_body() {
    let temp_dir = test_temp_dir("skill-load").await;
    let skill_dir = temp_dir.join(".night24").join("skills").join("review");
    tokio::fs::create_dir_all(&skill_dir).await.unwrap();
    tokio::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review changed code.\n---\n# Review\nFind bugs.\n",
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
            "run_id": "run-skill-load",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": working_dir,
                "conversation": []
            },
            "input": { "text": "tool:skill_load:review" },
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
    let output = core.handle_line(&request.to_string()).await;
    let joined = output.join("\n");
    assert!(joined.contains("developer__skill_load"));
    assert!(joined.contains("# Review"));
    assert!(joined.contains("Find bugs."));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn skill_load_tool_failure_events_precede_tool_response_message() {
    let temp_dir = test_temp_dir("skill-load-missing-events").await;
    let mut core = initialized_core().await;
    let working_dir = temp_dir.to_string_lossy().to_string();
    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-skill-load-missing",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": working_dir,
                "conversation": []
            },
            "input": { "text": "tool:skill_load:missing" },
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

    let output = core.handle_line(&request.to_string()).await;
    let events = agent_events(&output);

    let started_index = events
        .iter()
        .position(|event| event["params"]["type"] == "tool_started")
        .expect("tool_started event");
    let lifecycle_failed_index = events
        .iter()
        .position(|event| {
            event["params"]["type"] == "tool_failed"
                && event["params"]["payload"]["error"]["code"] == "skill_tool_failed"
        })
        .expect("skill lifecycle tool_failed event");
    let outer_failed_index = events
        .iter()
        .position(|event| {
            event["params"]["type"] == "tool_failed"
                && event["params"]["payload"]["error"]["code"] == "tool_execution_failed"
        })
        .expect("outer tool_failed event");
    let assistant_error_index = events
        .iter()
        .position(|event| {
            event["params"]["type"] == "message"
                && event["params"]["payload"]["message"]["role"] == "assistant"
                && event["params"]["payload"]["message"]["content"]
                    .as_array()
                    .is_some_and(|content| {
                        content.iter().any(|block| {
                            block["type"] == "text"
                                && block["text"]
                                    .as_str()
                                    .is_some_and(|text| text.contains("skill not found"))
                        })
                    })
        })
        .expect("assistant error message");
    let finish_index = events
        .iter()
        .position(|event| event["params"]["type"] == "finish")
        .expect("finish event");
    let finish_messages = events[finish_index]["params"]["payload"]["messages"]
        .as_array()
        .expect("finish messages");
    let finish_has_tool_response_error = finish_messages.iter().any(|message| {
        message["role"] == "tool"
            && message["content"].as_array().is_some_and(|content| {
                content.iter().any(|block| {
                    block["type"] == "tool_response"
                        && block["is_error"] == true
                        && block["content"]
                            .as_str()
                            .is_some_and(|text| text.contains("skill not found"))
                })
            })
    });

    assert!(started_index < lifecycle_failed_index);
    assert!(lifecycle_failed_index < outer_failed_index);
    assert!(outer_failed_index < assistant_error_index);
    assert!(assistant_error_index < finish_index);
    assert!(
        finish_has_tool_response_error,
        "finish messages: {}",
        serde_json::to_string_pretty(finish_messages).unwrap()
    );
    assert_eq!(
        events[started_index]["params"]["payload"]["tool_name"],
        "developer__skill_load"
    );
    assert_eq!(
        events[finish_index]["params"]["payload"]["status"],
        "completed"
    );

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn builtin_tool_failure_events_precede_tool_response_message() {
    let temp_dir = test_temp_dir("read-missing-events").await;
    let mut core = initialized_core().await;
    let working_dir = temp_dir.to_string_lossy().to_string();
    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-read-missing",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": working_dir,
                "conversation": []
            },
            "input": { "text": "tool:read:missing.txt" },
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

    let output = core.handle_line(&request.to_string()).await;
    let events = agent_events(&output);

    let started_index = events
        .iter()
        .position(|event| event["params"]["type"] == "tool_started")
        .expect("tool_started event");
    let lifecycle_failed_index = events
        .iter()
        .position(|event| {
            event["params"]["type"] == "tool_failed"
                && event["params"]["payload"]["error"]["code"] == "tool_failed"
        })
        .expect("builtin lifecycle tool_failed event");
    let outer_failed_index = events
        .iter()
        .position(|event| {
            event["params"]["type"] == "tool_failed"
                && event["params"]["payload"]["error"]["code"] == "tool_execution_failed"
        })
        .expect("outer tool_failed event");
    let finish_index = events
        .iter()
        .position(|event| event["params"]["type"] == "finish")
        .expect("finish event");
    let finish_messages = events[finish_index]["params"]["payload"]["messages"]
        .as_array()
        .expect("finish messages");
    let finish_has_tool_request_path = finish_messages.iter().any(|message| {
        message["role"] == "assistant"
            && message["content"].as_array().is_some_and(|content| {
                content.iter().any(|block| {
                    block["type"] == "tool_request"
                        && block["name"] == "developer__read_file"
                        && block["arguments"]["path"] == "missing.txt"
                })
            })
    });
    let finish_has_tool_response_error = finish_messages.iter().any(|message| {
        message["role"] == "tool"
            && message["content"].as_array().is_some_and(|content| {
                content.iter().any(|block| {
                    block["type"] == "tool_response"
                        && block["is_error"] == true
                        && block["content"]
                            .as_str()
                            .is_some_and(|text| text.starts_with("error:"))
                })
            })
    });

    assert!(started_index < lifecycle_failed_index);
    assert!(lifecycle_failed_index < outer_failed_index);
    assert!(outer_failed_index < finish_index);
    assert!(finish_has_tool_request_path);
    assert!(finish_has_tool_response_error);
    assert_eq!(
        events[started_index]["params"]["payload"]["tool_name"],
        "developer__read_file"
    );
    assert_eq!(
        events[finish_index]["params"]["payload"]["status"],
        "completed"
    );

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn subagent_sync_spawn_returns_child_result() {
    let mut core = initialized_core().await;
    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-subagent-sync",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": ".",
                "conversation": []
            },
            "input": { "text": "tool:subagent_sync: inspect docs" },
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

    let output = core.handle_line(&request.to_string()).await;
    let joined = output.join("\n");

    assert!(joined.contains("developer__subagent_spawn"));
    assert!(joined.contains("subagent-"));
    assert!(
        joined.contains("\"status\":\"completed\"") || joined.contains("\"status\": \"completed\"")
    );
    assert!(joined.contains("inspect docs"));
}

#[tokio::test]
async fn subagent_async_spawn_is_visible_in_pool_status() {
    let mut core = initialized_core().await;
    let spawn = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-subagent-async",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": ".",
                "conversation": []
            },
            "input": { "text": "tool:subagent_async: summarize files" },
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
    let spawn_output = core.handle_line(&spawn.to_string()).await;
    let spawn_text = spawn_output.join("\n");
    assert!(spawn_text.contains("subagent-"));

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let status = json!({
        "jsonrpc": "2.0",
        "id": "rpc-status",
        "method": "agent.reply",
        "params": {
            "run_id": "run-subagent-status",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": ".",
                "conversation": []
            },
            "input": { "text": "tool:subagent_status:" },
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

    let status_output = core.handle_line(&status.to_string()).await;
    let status_text = status_output.join("\n");
    assert!(status_text.contains("summarize files"));

    let pool = core.subagents.snapshot(None, true, true).unwrap();
    assert_eq!(pool["total"], 1);
    assert_eq!(pool["subagents"][0]["task"], "summarize files");
}

#[tokio::test]
async fn subagent_cancel_tool_marks_target_cancelled() {
    let mut core = initialized_core().await;
    let handle = core
        .subagents
        .create(
            "run-subagent-cancel-tool",
            "run-subagent-cancel-tool:child",
            Some("worker"),
            "long task",
            SubAgentMode::Async,
        )
        .unwrap();
    core.subagents.mark_running(&handle.id);

    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-subagent-cancel-tool",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": ".",
                "conversation": []
            },
            "input": { "text": format!("tool:subagent_cancel:{}", handle.id) },
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

    let output = core.handle_line(&request.to_string()).await;
    let events = agent_events(&output);

    let started_index = events
        .iter()
        .position(|event| {
            event["params"]["type"] == "tool_started"
                && event["params"]["payload"]["tool_name"] == "developer__subagent_cancel"
        })
        .expect("subagent cancel tool_started event");
    let finished_index = events
        .iter()
        .position(|event| {
            event["params"]["type"] == "tool_finished"
                && event["params"]["payload"]["tool_name"] == "developer__subagent_cancel"
        })
        .expect("subagent cancel tool_finished event");
    let finish_index = events
        .iter()
        .position(|event| event["params"]["type"] == "finish")
        .expect("finish event");
    assert!(started_index < finished_index);
    assert!(finished_index < finish_index);
    assert_eq!(
        events[finish_index]["params"]["payload"]["status"],
        "completed"
    );

    let snapshot = core
        .subagents
        .snapshot(Some(&handle.id), true, true)
        .unwrap();
    assert_eq!(snapshot["status"], "cancelled");
    assert_eq!(snapshot["task"], "long task");

    let finish_text = events[finish_index].to_string();
    assert!(finish_text.contains("developer__subagent_cancel"));
    assert!(finish_text.contains("cancelled"));
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
async fn wrong_run_permission_resolution_does_not_consume_request() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut core = AgentCore::with_output(tx);
    initialize_core(&mut core).await;

    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-permission-owned",
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

    let wrong_resolve = json!({
        "jsonrpc": "2.0",
        "id": "rpc-permission-wrong-run",
        "method": "permission.resolve",
        "params": {
            "run_id": "run-other",
            "permission_id": permission_id.clone(),
            "decision": "approve"
        }
    });
    let wrong_resolved = core.handle_line(&wrong_resolve.to_string()).await;
    let wrong_resolved: serde_json::Value = serde_json::from_str(&wrong_resolved[0]).unwrap();
    assert_eq!(
        wrong_resolved["error"]["message"],
        "permission request does not belong to run"
    );

    let resolve = json!({
        "jsonrpc": "2.0",
        "id": "rpc-permission",
        "method": "permission.resolve",
        "params": {
            "run_id": "run-permission-owned",
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
async fn denied_tool_permission_fails_without_starting_tool() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut core = AgentCore::with_output(tx);
    initialize_core(&mut core).await;

    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-permission-deny",
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
            "run_id": "run-permission-deny",
            "permission_id": permission_id,
            "decision": "deny"
        }
    });
    let resolved = core.handle_line(&resolve.to_string()).await;
    let resolved: serde_json::Value = serde_json::from_str(&resolved[0]).unwrap();
    assert_eq!(resolved["result"]["accepted"], true);

    let events = collect_events_until_finish(&mut rx).await;
    assert!(!events
        .iter()
        .any(|event| event["params"]["type"] == "tool_started"));

    let failed = events
        .iter()
        .find(|event| {
            event["params"]["type"] == "tool_failed"
                && event["params"]["payload"]["error"]["code"] == "tool_execution_failed"
        })
        .expect("tool_execution_failed event");
    assert_eq!(
        failed["params"]["payload"]["tool_name"],
        "developer__datetime"
    );
    assert!(failed["params"]["payload"]["error"]["message"]
        .as_str()
        .is_some_and(|message| message.contains("permission denied")));

    let finish = events
        .iter()
        .find(|event| event["params"]["type"] == "finish")
        .expect("finish event");
    assert_eq!(finish["params"]["payload"]["status"], "completed");
    let finish_messages = finish["params"]["payload"]["messages"]
        .as_array()
        .expect("finish messages");
    let has_denied_tool_response = finish_messages.iter().any(|message| {
        message["role"] == "tool"
            && message["content"].as_array().is_some_and(|content| {
                content.iter().any(|block| {
                    block["type"] == "tool_response"
                        && block["is_error"] == true
                        && block["content"].as_str().is_some_and(|text| {
                            text.contains("permission denied for developer__datetime")
                        })
                })
            })
    });
    assert!(has_denied_tool_response);
}

#[tokio::test]
async fn denied_subagent_tool_permission_fails_without_starting_tool() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut core = AgentCore::with_output(tx);
    initialize_core(&mut core).await;

    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-subagent-permission-deny",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": ".",
                "conversation": []
            },
            "input": { "text": "tool:subagent_async: summarize files" },
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
    assert_eq!(
        permission["params"]["payload"]["tool_name"],
        "developer__subagent_spawn"
    );
    let permission_id = permission["params"]["payload"]["permission_id"]
        .as_str()
        .unwrap()
        .to_string();

    let resolve = json!({
        "jsonrpc": "2.0",
        "id": "rpc-permission",
        "method": "permission.resolve",
        "params": {
            "run_id": "run-subagent-permission-deny",
            "permission_id": permission_id,
            "decision": "deny"
        }
    });
    let resolved = core.handle_line(&resolve.to_string()).await;
    let resolved: serde_json::Value = serde_json::from_str(&resolved[0]).unwrap();
    assert_eq!(resolved["result"]["accepted"], true);

    let events = collect_events_until_finish(&mut rx).await;
    assert!(!events
        .iter()
        .any(|event| event["params"]["type"] == "tool_started"));
    assert_eq!(
        core.subagents.snapshot(None, false, false).unwrap()["total"],
        0
    );

    let failed = events
        .iter()
        .find(|event| {
            event["params"]["type"] == "tool_failed"
                && event["params"]["payload"]["error"]["code"] == "tool_execution_failed"
        })
        .expect("tool_execution_failed event");
    assert_eq!(
        failed["params"]["payload"]["tool_name"],
        "developer__subagent_spawn"
    );
    assert!(failed["params"]["payload"]["error"]["message"]
        .as_str()
        .is_some_and(|message| message.contains("permission denied")));

    let finish = events
        .iter()
        .find(|event| event["params"]["type"] == "finish")
        .expect("finish event");
    assert_eq!(finish["params"]["payload"]["status"], "completed");
    let finish_messages = finish["params"]["payload"]["messages"]
        .as_array()
        .expect("finish messages");
    let has_denied_tool_response = finish_messages.iter().any(|message| {
        message["role"] == "tool"
            && message["content"].as_array().is_some_and(|content| {
                content.iter().any(|block| {
                    block["type"] == "tool_response"
                        && block["is_error"] == true
                        && block["content"].as_str().is_some_and(|text| {
                            text.contains("permission denied for developer__subagent_spawn")
                        })
                })
            })
    });
    assert!(has_denied_tool_response);
}

#[tokio::test]
async fn denied_skill_tool_permission_fails_without_starting_tool() {
    let temp_dir = test_temp_dir("skill-permission-deny").await;
    let skill_dir = temp_dir.join(".night24").join("skills").join("review");
    tokio::fs::create_dir_all(&skill_dir).await.unwrap();
    tokio::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review changed code.\n---\n# Review\nFind bugs.\n",
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
            "run_id": "run-skill-permission-deny",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": working_dir,
                "conversation": []
            },
            "input": { "text": "tool:skill_load:review" },
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
    assert_eq!(
        permission["params"]["payload"]["tool_name"],
        "developer__skill_load"
    );
    let permission_id = permission["params"]["payload"]["permission_id"]
        .as_str()
        .unwrap()
        .to_string();

    let resolve = json!({
        "jsonrpc": "2.0",
        "id": "rpc-permission",
        "method": "permission.resolve",
        "params": {
            "run_id": "run-skill-permission-deny",
            "permission_id": permission_id,
            "decision": "deny"
        }
    });
    let resolved = core.handle_line(&resolve.to_string()).await;
    let resolved: serde_json::Value = serde_json::from_str(&resolved[0]).unwrap();
    assert_eq!(resolved["result"]["accepted"], true);

    let events = collect_events_until_finish(&mut rx).await;
    assert!(!events
        .iter()
        .any(|event| event["params"]["type"] == "tool_started"));

    let failed = events
        .iter()
        .find(|event| {
            event["params"]["type"] == "tool_failed"
                && event["params"]["payload"]["error"]["code"] == "tool_execution_failed"
        })
        .expect("tool_execution_failed event");
    assert_eq!(
        failed["params"]["payload"]["tool_name"],
        "developer__skill_load"
    );
    assert!(failed["params"]["payload"]["error"]["message"]
        .as_str()
        .is_some_and(|message| message.contains("permission denied")));

    let finish = events
        .iter()
        .find(|event| event["params"]["type"] == "finish")
        .expect("finish event");
    assert_eq!(finish["params"]["payload"]["status"], "completed");
    let finish_text = finish.to_string();
    assert!(finish_text.contains("permission denied for developer__skill_load"));
    assert!(!finish_text.contains("# Review"));
    assert!(!finish_text.contains("Find bugs."));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
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
async fn cancel_running_builtin_tool_finishes_cancelled() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut core = AgentCore::with_output(tx);
    initialize_core(&mut core).await;

    let command = if cfg!(target_os = "windows") {
        "ping -n 2 127.0.0.1 > nul"
    } else {
        "sleep 1"
    };
    let request = json!({
        "jsonrpc": "2.0",
        "id": "rpc-reply",
        "method": "agent.reply",
        "params": {
            "run_id": "run-cancel-running-tool",
            "session": {
                "id": "session-1",
                "name": "test",
                "working_dir": ".",
                "conversation": []
            },
            "input": { "text": format!("tool:shell:{command}") },
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
                "permission_mode": "allow_all"
            }
        }
    });

    let accepted = core.handle_line(&request.to_string()).await;
    let accepted: serde_json::Value = serde_json::from_str(&accepted[0]).unwrap();
    assert_eq!(accepted["result"]["accepted"], true);

    let tool_started = next_event_of_type(&mut rx, "tool_started").await;
    assert_eq!(
        tool_started["params"]["payload"]["tool_name"],
        "developer__shell"
    );

    let cancel = json!({
        "jsonrpc": "2.0",
        "id": "rpc-cancel",
        "method": "agent.cancel",
        "params": {
            "run_id": "run-cancel-running-tool",
            "reason": "test"
        }
    });
    let cancelled = core.handle_line(&cancel.to_string()).await;
    let cancelled: serde_json::Value = serde_json::from_str(&cancelled[0]).unwrap();
    assert_eq!(cancelled["result"]["accepted"], true);

    let events = collect_events_until_finish(&mut rx).await;
    assert!(!events
        .iter()
        .any(|event| event["params"]["type"] == "tool_finished"));

    let failed = events
        .iter()
        .find(|event| {
            event["params"]["type"] == "tool_failed"
                && event["params"]["payload"]["error"]["code"] == "cancelled"
        })
        .expect("cancelled tool_failed event");
    assert_eq!(failed["params"]["payload"]["tool_name"], "developer__shell");

    let finish = events
        .iter()
        .find(|event| event["params"]["type"] == "finish")
        .expect("finish event");
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
    let hook_dir = temp_dir.join(".night24");
    tokio::fs::create_dir_all(&hook_dir).await.unwrap();
    tokio::fs::write(
        hook_dir.join("hooks.json"),
        r#"{
            "hooks": [
                {
                    "event": "permission_required",
                    "name": "permission-audit",
                    "engine": "gts",
                    "inline_script": "function execute(args) { return { outputs: [{ stream: \"stdout\", text: \"permission-hook tool=\" + args.tool_name + \" source=\" + args.arguments.source_tool }] }; }"
                }
            ]
        }"#,
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

    let hook_output = next_event_of_type(&mut rx, "run_output").await;
    assert_eq!(
        hook_output["params"]["payload"]["source"],
        "hook:permission_required:permission-audit"
    );
    assert_eq!(
        hook_output["params"]["payload"]["text"],
        "permission-hook tool=developer__sensitive_output source=developer__read_file"
    );
    assert!(!hook_output.to_string().contains("sk-test1234567890abcdef"));
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
fn permission_manager_mode_aliases_use_shared_normalization() {
    assert_eq!(
        permission_manager_for_mode(Some("allow-all")).check_sync("developer__shell"),
        night24_core::permission::PermissionLevel::Allow
    );
    assert_eq!(
        permission_manager_for_mode(Some("full_access")).check_sync("developer__write_file"),
        night24_core::permission::PermissionLevel::Allow
    );
    assert_eq!(
        permission_manager_for_mode(Some("deny-all")).check_sync("developer__read_file"),
        night24_core::permission::PermissionLevel::Deny
    );
    assert_eq!(
        permission_manager_for_mode(Some("permissive")).check_sync("developer__read_file"),
        night24_core::permission::PermissionLevel::Allow
    );
    assert_eq!(
        permission_manager_for_mode(None).check_sync("developer__read_file"),
        night24_core::permission::PermissionLevel::Confirm
    );
    assert_eq!(
        permission_manager_for_mode(Some("unknown")).check_sync("developer__read_file"),
        night24_core::permission::PermissionLevel::Confirm
    );
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

#[test]
fn hook_config_rejects_legacy_goscript_engine_alias() {
    let err = HookRunner::from_config_str(
        r#"{
            "hooks": [
                {
                    "event": "run_started",
                    "engine": "goscript",
                    "inline_script": "function execute(args) { return { outputs: [] }; }"
                }
            ]
        }"#,
    )
    .unwrap_err();

    assert!(err.to_string().contains("goscript"));
}

#[test]
fn hook_runner_loads_workspace_default_config() {
    let temp_dir =
        std::env::temp_dir().join(format!("night24-hooks-default-{}", uuid::Uuid::new_v4()));
    let config_dir = temp_dir.join(".night24");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("hooks.json"),
        r#"{
            "hooks": [
                {
                    "event": "run_started",
                    "name": "workspace-default",
                    "engine": "gts",
                    "inline_script": "function execute(args) { return { outputs: [] }; }"
                }
            ]
        }"#,
    )
    .unwrap();

    let runner = HookRunner::from_environment(&temp_dir);
    assert!(!runner.is_empty());

    let _ = std::fs::remove_dir_all(temp_dir);
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
async fn gts_hook_reports_malformed_structured_outputs_as_stderr() {
    let temp_dir = test_temp_dir("gts-malformed-output-hook").await;
    let config = serde_json::json!({
        "hooks": [
            {
                "event": "run_started",
                "name": "bad-array",
                "engine": "gts",
                "inline_script": "function execute(args) { return { outputs: \"not-an-array\" }; }",
                "timeout_ms": 5000
            },
            {
                "event": "run_started",
                "name": "bad-items",
                "engine": "gts",
                "inline_script": r#"function execute(args) {
                  return {
                    outputs: [
                      { stream: "telemetry", text: "ignored" },
                      { stream: "stdout", text: 42 },
                      { stream: "stderr" },
                      "bad-item"
                    ]
                  };
                }"#,
                "timeout_ms": 5000
            }
        ]
    });
    let runner = HookRunner::from_config_str(&config.to_string()).unwrap();

    let outputs = runner
        .run(&HookContext {
            event: HookEvent::RunStarted,
            run_id: "run-gts-malformed",
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

    assert_eq!(outputs.len(), 5);
    assert!(outputs
        .iter()
        .all(|output| matches!(output.stream, night24_protocol::OutputStream::Stderr)));
    let stderr = outputs
        .iter()
        .map(|output| output.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(stderr.contains("outputs must be an array"));
    assert!(stderr.contains("outputs[0].stream must be stdout or stderr, got telemetry"));
    assert!(stderr.contains("outputs[1].text must be a string"));
    assert!(stderr.contains("outputs[2].text is required"));
    assert!(stderr.contains("outputs[3] must be an object"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn gts_hook_runs_top_level_before_execute() {
    let temp_dir = test_temp_dir("gts-top-level-hook").await;
    let config = serde_json::json!({
        "hooks": [
            {
                "event": "run_started",
                "name": "lifecycle",
                "engine": "gts",
                "inline_script": r#"println("top-level");
function execute(args) {
  return {
    outputs: [
      { stream: "stdout", text: "execute=" + args.run_id }
    ]
  };
}"#,
                "timeout_ms": 5000
            }
        ]
    });
    let runner = HookRunner::from_config_str(&config.to_string()).unwrap();

    let outputs = runner
        .run(&HookContext {
            event: HookEvent::RunStarted,
            run_id: "run-gts-lifecycle",
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
        .collect::<Vec<_>>();
    assert_eq!(stdout, vec!["top-level", "execute=run-gts-lifecycle"]);

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn gts_hook_timeout_releases_worker_for_next_hook() {
    let temp_dir = test_temp_dir("gts-shared-deadline-hook").await;
    let config = serde_json::json!({
        "hooks": [
            {
                "event": "run_started",
                "name": "slow-execute",
                "engine": "gts",
                "inline_script": r#"let started = Date().getTime();
while (Date().getTime() - started < 80) {}

function execute(args) {
  while (true) {}
}"#,
                "timeout_ms": 120,
                "instruction_limit": 100000000
            },
            {
                "event": "run_started",
                "name": "after-timeout",
                "engine": "gts",
                "inline_script": r#"function execute(args) {
  return { outputs: [{ stream: "stdout", text: "second-ok" }] };
}"#,
                "timeout_ms": 120
            }
        ]
    });
    let runner = HookRunner::from_config_str(&config.to_string()).unwrap();

    let outputs = runner
        .run(&HookContext {
            event: HookEvent::RunStarted,
            run_id: "run-gts-shared-deadline",
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

    let stderr = outputs
        .iter()
        .filter(|output| matches!(output.stream, night24_protocol::OutputStream::Stderr))
        .map(|output| output.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        stderr.contains("Timeout") || stderr.contains("timed out"),
        "expected timeout output, got: {stderr}"
    );

    let stdout = outputs
        .iter()
        .filter(|output| matches!(output.stream, night24_protocol::OutputStream::Stdout))
        .map(|output| output.text.as_str())
        .collect::<Vec<_>>();
    assert!(stdout.contains(&"second-ok"), "stdout was: {stdout:?}");

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

#[tokio::test]
async fn gts_hook_allowed_modules_can_reject_dangerous_modules() {
    let temp_dir = test_temp_dir("gts-allowlist-hook").await;
    let config = serde_json::json!({
        "hooks": [
            {
                "event": "run_started",
                "name": "restricted",
                "engine": "gts",
                "inline_script": r#"let exec = require("@std/exec");
function execute(args) {
  return { outputs: [{ stream: "stdout", text: "unexpected" }] };
}"#,
                "allowed_modules": [],
                "timeout_ms": 5000
            }
        ]
    });
    let runner = HookRunner::from_config_str(&config.to_string()).unwrap();

    let outputs = runner
        .run(&HookContext {
            event: HookEvent::RunStarted,
            run_id: "run-gts-allowlist",
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

    assert_eq!(outputs.len(), 1);
    assert!(matches!(
        outputs[0].stream,
        night24_protocol::OutputStream::Stderr
    ));
    assert!(outputs[0]
        .text
        .contains("module '@std/exec' is not allowed"));

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

async fn collect_events_until_finish(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
) -> Vec<serde_json::Value> {
    let mut events = Vec::new();
    for _ in 0..20 {
        let raw = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for agent event")
            .expect("agent event channel closed");
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let is_finish = value["params"]["type"] == "finish";
        events.push(value);
        if is_finish {
            return events;
        }
    }
    panic!("finish event was not emitted");
}

fn agent_events(output: &[String]) -> Vec<serde_json::Value> {
    output
        .iter()
        .filter_map(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .filter(|value| value["method"] == "agent.event")
        .collect()
}
