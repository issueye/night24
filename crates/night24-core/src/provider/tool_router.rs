use crate::model::{ContentBlock, Message, Role};
use crate::security::SecurityInspector;
use crate::tool_executor::execute_tool;
use uuid::Uuid;

pub async fn route_tool_input(tool_input: &str) -> Option<Message> {
    if tool_input.starts_with("shell:") {
        let command = tool_input.trim_start_matches("shell:").trim().to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__shell".to_string(),
                arguments: serde_json::json!({"command": command}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("read:") {
        let path = tool_input.trim_start_matches("read:").trim().to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__read_file".to_string(),
                arguments: serde_json::json!({"path": path}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("write:") {
        let rest = tool_input.trim_start_matches("write:").trim().to_string();
        let mut parts = rest.splitn(2, ' ');
        let path = parts.next().unwrap_or("").to_string();
        let content = parts.next().unwrap_or("").to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__write_file".to_string(),
                arguments: serde_json::json!({"path": path, "content": content}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("list_files:") {
        let rest = tool_input
            .trim_start_matches("list_files:")
            .trim()
            .to_string();
        let mut parts = rest.splitn(2, ' ');
        let path = parts.next().unwrap_or(".").to_string();
        let recursive = parts.next().map(|v| v == "true").unwrap_or(false);
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__list_files".to_string(),
                arguments: serde_json::json!({"path": path, "recursive": recursive}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("datetime:") {
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__datetime".to_string(),
                arguments: serde_json::json!({}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("calculator:") {
        let expression = tool_input
            .trim_start_matches("calculator:")
            .trim()
            .to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__calculator".to_string(),
                arguments: serde_json::json!({"expression": expression}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("http:") {
        let url = tool_input.trim_start_matches("http:").trim().to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__http_request".to_string(),
                arguments: serde_json::json!({"url": url, "method": "GET"}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("web_search:") {
        let query = tool_input
            .trim_start_matches("web_search:")
            .trim()
            .to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__web_search".to_string(),
                arguments: serde_json::json!({"query": query}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("jq:") {
        let query = tool_input.trim_start_matches("jq:").trim().to_string();
        let data = r#"{"name":"night24","version":"0.1.0"}"#.to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__jq".to_string(),
                arguments: serde_json::json!({"data": data, "query": query}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("file_search:") {
        let query = tool_input
            .trim_start_matches("file_search:")
            .trim()
            .to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__file_search".to_string(),
                arguments: serde_json::json!({"query": query, "path": "."}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("web_scraper:") {
        let url = tool_input
            .trim_start_matches("web_scraper:")
            .trim()
            .to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__web_scraper".to_string(),
                arguments: serde_json::json!({"url": url}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("code_interpreter:") {
        let rest = tool_input
            .trim_start_matches("code_interpreter:")
            .trim()
            .to_string();
        let mut parts = rest.splitn(2, ' ');
        let language = parts.next().unwrap_or("python").to_string();
        let code = parts.next().unwrap_or("").to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__code_interpreter".to_string(),
                arguments: serde_json::json!({"code": code, "language": language}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("database_query:") {
        let query = tool_input
            .trim_start_matches("database_query:")
            .trim()
            .to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__database_query".to_string(),
                arguments: serde_json::json!({"query": query}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("subagent_sync:") {
        let task = tool_input
            .trim_start_matches("subagent_sync:")
            .trim()
            .to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__subagent_spawn".to_string(),
                arguments: serde_json::json!({"task": task, "mode": "sync", "provider": "echo", "model": "echo-v1", "max_turns": 1, "timeout_ms": 10000}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("subagent_async:") {
        let task = tool_input
            .trim_start_matches("subagent_async:")
            .trim()
            .to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__subagent_spawn".to_string(),
                arguments: serde_json::json!({"task": task, "mode": "async", "provider": "echo", "model": "echo-v1", "max_turns": 1, "timeout_ms": 10000}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("subagent_status:") {
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__subagent_status".to_string(),
                arguments: serde_json::json!({"include_result": true, "include_messages": true}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("subagent_cancel:") {
        let id = tool_input
            .trim_start_matches("subagent_cancel:")
            .trim()
            .to_string();
        let arguments = if id.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::json!({"subagent_id": id})
        };
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__subagent_cancel".to_string(),
                arguments,
            }],
            created_at: chrono::Utc::now(),
        });
    }

    if tool_input.starts_with("skill_load:") {
        let name = tool_input
            .trim_start_matches("skill_load:")
            .trim()
            .to_string();
        return Some(Message {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: Uuid::new_v4().to_string(),
                name: "developer__skill_load".to_string(),
                arguments: serde_json::json!({"name": name}),
            }],
            created_at: chrono::Utc::now(),
        });
    }

    None
}

pub async fn execute_echo_tool(tool_input: &str, _security: &SecurityInspector) -> Option<Message> {
    let security = SecurityInspector::new(std::sync::Arc::new(
        crate::permission::PermissionManager::default(),
    ));
    let result = execute_tool(
        "developer__echo",
        &serde_json::json!({"text": tool_input}),
        std::path::Path::new("."),
        &security,
    )
    .await;
    let content = result.unwrap_or_else(|e| format!("tool error: {}", e));

    Some(Message {
        id: Uuid::new_v4().to_string(),
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: format!("[echo] {}", content),
        }],
        created_at: chrono::Utc::now(),
    })
}
