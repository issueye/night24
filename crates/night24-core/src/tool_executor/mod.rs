mod definitions;
mod filesystem;
mod network;

use std::path::Path;
use std::time::Duration;

use sqlx::{Column, Row};
use tokio::time::timeout;

use crate::permission::PermissionLevel;
use crate::security::SecurityInspector;

pub use definitions::builtin_tools;
use filesystem::{glob_match, resolve_within_workdir, run_shell_command, should_skip_dir};
#[cfg(test)]
use network::{
    clean_duckduckgo_results, configured_network_proxy, format_duckduckgo_results, http_client,
    validate_http_url, MAX_SEARCH_SNIPPET_CHARS,
};
use network::{
    fetch_network_body, html_to_text, parse_headers, proxy_from_arguments, search_web,
    send_network_request, truncate_chars, MAX_NETWORK_RESPONSE_CHARS,
};

pub async fn execute_tool(
    name: &str,
    arguments: &serde_json::Value,
    working_dir: &Path,
    security: &SecurityInspector,
) -> anyhow::Result<String> {
    execute_tool_inner(name, arguments, working_dir, security, true).await
}

pub async fn execute_tool_raw_output(
    name: &str,
    arguments: &serde_json::Value,
    working_dir: &Path,
    security: &SecurityInspector,
) -> anyhow::Result<String> {
    execute_tool_inner(name, arguments, working_dir, security, false).await
}

async fn execute_tool_inner(
    name: &str,
    arguments: &serde_json::Value,
    working_dir: &Path,
    security: &SecurityInspector,
    sanitize_read_output: bool,
) -> anyhow::Result<String> {
    timeout(Duration::from_secs(10), async {
        match name {
            "developer__echo" => {
                let text = arguments.get("text").and_then(|v| v.as_str()).unwrap_or("");
                Ok(text.to_string())
            }
            "developer__shell" => {
                let command = arguments
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing `command` for developer__shell"))?;

                let findings = security.inspect_input(command).await;
                if !findings.is_empty() {
                    return Ok(format!("security inspection: {}", findings.join("; ")));
                }

                let permission = security.require_permission("developer__shell").await;
                if permission == PermissionLevel::Deny {
                    return Ok("permission denied for developer__shell".to_string());
                }

                run_shell_command(command, working_dir).await
            }
            "developer__read_file" => {
                let path = arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing `path` for developer__read_file"))?;

                let permission = security.require_permission("developer__read_file").await;
                if permission == PermissionLevel::Deny {
                    return Ok("permission denied for developer__read_file".to_string());
                }

                let resolved = resolve_within_workdir(working_dir, path)?;
                let content = tokio::fs::read_to_string(resolved).await?;

                if sanitize_read_output {
                    let inspection = security.sanitize_output(&content).await;
                    if !inspection.findings.is_empty() {
                        return Ok(format!(
                            "security inspection: {}\n\n{}",
                            inspection.findings.join("; "),
                            inspection.sanitized
                        ));
                    }
                }

                Ok(content)
            }
            "developer__write_file" => {
                let path = arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing `path` for developer__write_file"))?;

                let content = arguments
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        anyhow::anyhow!("missing `content` for developer__write_file")
                    })?;

                let permission = security.require_permission("developer__write_file").await;
                if permission == PermissionLevel::Deny {
                    return Ok("permission denied for developer__write_file".to_string());
                }

                let resolved = resolve_within_workdir(working_dir, path)?;

                if let Some(parent) = resolved.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }

                tokio::fs::write(resolved, content).await?;
                Ok("file written".to_string())
            }
            "developer__list_files" => {
                let target = arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| ".".to_string());

                let recursive = arguments
                    .get("recursive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let permission = security.require_permission("developer__list_files").await;
                if permission == PermissionLevel::Deny {
                    return Ok("permission denied for developer__list_files".to_string());
                }

                let resolved = resolve_within_workdir(working_dir, &target)?;

                if recursive {
                    let mut entries = Vec::new();
                    let mut walker = tokio::fs::read_dir(resolved).await?;
                    while let Some(entry) = walker.next_entry().await? {
                        entries.push(entry.path().to_string_lossy().to_string());
                    }
                    Ok(entries.join("\n"))
                } else {
                    let mut entries = Vec::new();
                    let mut walker = tokio::fs::read_dir(resolved).await?;
                    while let Some(entry) = walker.next_entry().await? {
                        entries.push(entry.file_name().to_string_lossy().to_string());
                    }
                    Ok(entries.join("\n"))
                }
            }
            "developer__datetime" => {
                let now = chrono::Utc::now();
                Ok(format!("utc={}", now.to_rfc3339()))
            }
            "developer__calculator" => {
                let expression = arguments
                    .get("expression")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        anyhow::anyhow!("missing `expression` for developer__calculator")
                    })?;

                let sanitized = expression
                    .chars()
                    .filter(|c| c.is_ascii_digit() || "+-*/().".contains(*c) || c.is_whitespace())
                    .collect::<String>();

                let value = meval::eval_str(&sanitized)
                    .map_err(|e| anyhow::anyhow!("calculator error: {}", e))?;
                Ok(format!("{}", value))
            }
            "developer__http_request" | "developer__network_request" => {
                let url = arguments
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing `url` for network request"))?;

                let method = arguments
                    .get("method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("GET");

                let body = arguments
                    .get("body")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                send_network_request(
                    url,
                    method,
                    parse_headers(arguments),
                    body,
                    proxy_from_arguments(arguments).as_deref(),
                )
                .await
            }
            "developer__web_search" | "developer__network_search" => {
                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing `query` for web search"))?;

                search_web(query, proxy_from_arguments(arguments).as_deref()).await
            }
            "developer__jq" => {
                let data = arguments
                    .get("data")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing `data` for developer__jq"))?;

                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing `query` for developer__jq"))?;

                let parsed: serde_json::Value = serde_json::from_str(data)
                    .map_err(|e| anyhow::anyhow!("invalid json input: {}", e))?;

                if query.trim() == "." {
                    return Ok(serde_json::to_string_pretty(&parsed)?);
                }

                if query.trim() == "keys" {
                    if let serde_json::Value::Object(map) = &parsed {
                        return Ok(map.keys().cloned().collect::<Vec<_>>().join("\n"));
                    }
                    return Ok("(not an object)".to_string());
                }

                if query.trim().starts_with(".[") {
                    let index_str = query.trim().trim_start_matches(".[").trim_end_matches(']');
                    if let Ok(index) = index_str.parse::<usize>() {
                        if let serde_json::Value::Array(arr) = &parsed {
                            if let Some(item) = arr.get(index) {
                                return Ok(serde_json::to_string_pretty(item)?);
                            }
                            return Ok("(index out of range)".to_string());
                        }
                        return Ok("(not an array)".to_string());
                    }
                }

                Ok(format!("(unsupported jq-like query in Phase 1: {})", query))
            }
            "developer__file_search" => {
                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing `query` for developer__file_search"))?;

                let target = arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| ".".to_string());

                let file_pattern = arguments.get("file_pattern").and_then(|v| v.as_str());

                let resolved = resolve_within_workdir(working_dir, &target)?;
                let mut matches = Vec::new();

                if resolved.is_file() {
                    let content = tokio::fs::read_to_string(&resolved).await?;
                    if content.contains(query) {
                        matches.push(resolved.to_string_lossy().to_string());
                    }
                } else {
                    let mut stack = vec![resolved];
                    while let Some(current) = stack.pop() {
                        if should_skip_dir(&current) {
                            continue;
                        }
                        let mut walker = match tokio::fs::read_dir(&current).await {
                            Ok(w) => w,
                            Err(_) => continue,
                        };
                        while let Some(entry) = walker.next_entry().await? {
                            let path = entry.path();
                            if path.is_dir() {
                                stack.push(path);
                            } else if path.is_file() {
                                if let Some(pattern) = file_pattern {
                                    let name =
                                        path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                                    if !glob_match(pattern, name) {
                                        continue;
                                    }
                                }
                                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                                    if content.contains(query) {
                                        matches.push(path.to_string_lossy().to_string());
                                    }
                                }
                            }
                        }
                    }
                }

                if matches.is_empty() {
                    Ok("(no matches found)".to_string())
                } else {
                    Ok(matches.join("\n"))
                }
            }
            "developer__web_scraper" => {
                let url = arguments
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing `url` for developer__web_scraper"))?;

                let html =
                    fetch_network_body(url, proxy_from_arguments(arguments).as_deref()).await?;

                Ok(truncate_chars(
                    &html_to_text(&html),
                    MAX_NETWORK_RESPONSE_CHARS,
                ))
            }
            "developer__code_interpreter" => {
                let code = arguments
                    .get("code")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        anyhow::anyhow!("missing `code` for developer__code_interpreter")
                    })?;

                let language = arguments
                    .get("language")
                    .and_then(|v| v.as_str())
                    .unwrap_or("python")
                    .to_lowercase();

                let escaped = code.replace("\\", "\\\\").replace("\"", "\\\"");
                let output = match language.as_str() {
                    "python" => {
                        run_shell_command(&format!("python -u -c \"{}\"", escaped), working_dir)
                            .await?
                    }
                    "javascript" | "js" => {
                        run_shell_command(&format!("node -e \"{}\"", escaped), working_dir).await?
                    }
                    _ => anyhow::bail!("unsupported language: {}", language),
                };

                Ok(output)
            }
            "developer__database_query" => {
                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        anyhow::anyhow!("missing `query` for developer__database_query")
                    })?;

                let sanitized = query.trim().to_lowercase();
                if sanitized.starts_with("drop")
                    || sanitized.starts_with("delete")
                    || sanitized.starts_with("update")
                    || sanitized.starts_with("insert")
                    || sanitized.starts_with("alter")
                {
                    anyhow::bail!("read-only query required; write operations are not allowed");
                }

                let db_path = working_dir.join("night24.db");
                if !db_path.exists() {
                    anyhow::bail!("database not found at {}", db_path.display());
                }

                let db_url = format!("sqlite:{}", db_path.display());
                let pool = sqlx::sqlite::SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect(&db_url)
                    .await?;

                let rows = sqlx::query(query).fetch_all(&pool).await?;
                let mut result = Vec::new();
                for row in rows {
                    let mut obj = serde_json::Map::new();
                    for col in row.columns() {
                        let name = col.name();
                        if let Ok(val) = row.try_get::<String, _>(name) {
                            obj.insert(name.to_string(), serde_json::Value::String(val));
                        } else if let Ok(val) = row.try_get::<i64, _>(name) {
                            obj.insert(name.to_string(), serde_json::Value::Number(val.into()));
                        } else if let Ok(val) = row.try_get::<f64, _>(name) {
                            if let Some(n) = serde_json::Number::from_f64(val) {
                                obj.insert(name.to_string(), serde_json::Value::Number(n));
                            }
                        } else if let Ok(val) = row.try_get::<bool, _>(name) {
                            obj.insert(name.to_string(), serde_json::Value::Bool(val));
                        } else if let Ok(val) = row.try_get::<Option<String>, _>(name) {
                            if let Some(v) = val {
                                obj.insert(name.to_string(), serde_json::Value::String(v));
                            } else {
                                obj.insert(name.to_string(), serde_json::Value::Null);
                            }
                        } else {
                            obj.insert(name.to_string(), serde_json::Value::Null);
                        }
                    }
                    result.push(serde_json::Value::Object(obj));
                }
                Ok(serde_json::to_string_pretty(&serde_json::Value::Array(
                    result,
                ))?)
            }
            _ => anyhow::bail!("unknown tool: {}", name),
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("tool execution timed out: {}", name))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::PermissionManager;
    use std::path::PathBuf;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn test_echo_tool() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"text": "hello"});
        let result = execute_tool(
            "developer__echo",
            &args,
            PathBuf::from(".").as_path(),
            &security,
        )
        .await;
        assert_eq!(result.unwrap(), "hello");
    }

    #[tokio::test]
    async fn test_datetime_tool() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({});
        let result = execute_tool(
            "developer__datetime",
            &args,
            PathBuf::from(".").as_path(),
            &security,
        )
        .await;
        assert!(result.unwrap().starts_with("utc=20"));
    }

    #[tokio::test]
    async fn test_calculator_tool() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"expression": "2 + 3 * 4"});
        let result = execute_tool(
            "developer__calculator",
            &args,
            PathBuf::from(".").as_path(),
            &security,
        )
        .await;
        assert_eq!(result.unwrap(), "14");
    }

    #[tokio::test]
    async fn test_jq_tool() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"data": "{\"a\":1,\"b\":2}", "query": "keys"});
        let result = execute_tool(
            "developer__jq",
            &args,
            PathBuf::from(".").as_path(),
            &security,
        )
        .await;
        assert!(result.unwrap().contains("a"));
    }

    #[test]
    fn test_web_search_result_formatting() {
        let value = serde_json::json!({
            "Heading": "Rust",
            "AbstractText": "Rust   is &quot;a&quot; programming language.",
            "AbstractURL": "https://www.rust-lang.org/",
            "RelatedTopics": [
                {
                    "Text": "Cargo is Rust's package manager.",
                    "FirstURL": "https://doc.rust-lang.org/cargo/"
                }
            ]
        });
        let result = format_duckduckgo_results("rust", &value);
        assert!(result.contains("Search results for: rust"));
        assert!(result.contains("Rust is \"a\" programming language."));
        assert!(result.contains("https://www.rust-lang.org/"));
    }

    #[test]
    fn test_web_search_cleaning_deduplicates_and_truncates() {
        let long_text = "x".repeat(400);
        let value = serde_json::json!({
            "RelatedTopics": [
                {
                    "Text": format!("Long topic - {}", long_text),
                    "FirstURL": "https://example.com/a"
                },
                {
                    "Text": "Duplicate topic - should be removed",
                    "FirstURL": "https://example.com/a"
                }
            ]
        });
        let cleaned = clean_duckduckgo_results("query", &value);
        assert_eq!(cleaned.len(), 1);
        assert_eq!(cleaned[0].title, "Long topic");
        assert!(cleaned[0].snippet.ends_with("..."));
        assert!(cleaned[0].snippet.chars().count() <= MAX_SEARCH_SNIPPET_CHARS + 3);
    }

    #[test]
    fn test_proxy_argument_and_direct_mode() {
        let args = serde_json::json!({"proxy": " http://127.0.0.1:8080 "});
        assert_eq!(
            proxy_from_arguments(&args).as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(configured_network_proxy(Some("direct")), None);
        assert!(http_client(Some("http://127.0.0.1:8080")).is_ok());
        assert!(http_client(Some("not a proxy")).is_err());
    }

    #[test]
    fn test_validate_http_url_rejects_unsupported_scheme() {
        let err = validate_http_url("file:///etc/passwd").unwrap_err();
        assert!(err.to_string().contains("unsupported url scheme"));
    }

    #[test]
    fn test_network_tool_definitions_include_aliases() {
        let names = builtin_tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"developer__http_request".to_string()));
        assert!(names.contains(&"developer__network_request".to_string()));
        assert!(names.contains(&"developer__web_search".to_string()));
        assert!(names.contains(&"developer__network_search".to_string()));
    }

    #[tokio::test]
    async fn test_shell_security_inspection() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"command": "rm -rf /"});
        let result = execute_tool(
            "developer__shell",
            &args,
            PathBuf::from(".").as_path(),
            &security,
        )
        .await;
        assert!(result.unwrap().contains("security inspection"));
    }

    #[tokio::test]
    async fn test_unknown_tool() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({});
        let result = execute_tool(
            "developer__unknown",
            &args,
            PathBuf::from(".").as_path(),
            &security,
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("unknown tool"));
    }

    #[tokio::test]
    async fn test_file_search_tool() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"query": "developer__echo", "path": "."});
        let result = execute_tool(
            "developer__file_search",
            &args,
            PathBuf::from(".").as_path(),
            &security,
        )
        .await;
        let output = result.unwrap();
        assert!(output.contains("tool_executor"));
    }

    #[tokio::test]
    async fn test_read_file_redacts_sensitive_output_instead_of_blocking() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let temp_dir =
            std::env::temp_dir().join(format!("night24-read-file-redact-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        tokio::fs::write(
            temp_dir.join("PRD.md"),
            "Project notes\nOPENAI_API_KEY=sk-test1234567890abcdef\nToken means session state",
        )
        .await
        .unwrap();

        let args = serde_json::json!({"path": "PRD.md"});
        let result = execute_tool("developer__read_file", &args, temp_dir.as_path(), &security)
            .await
            .unwrap();

        assert!(result.contains("security inspection:"));
        assert!(result.contains("Project notes"));
        assert!(result.contains("Token means session state"));
        assert!(result.contains("OPENAI_API_KEY=[redacted sensitive value]"));
        assert!(!result.contains("sk-test1234567890abcdef"));

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }

    #[test]
    fn test_html_to_text_strips_tags() {
        // Mirrors the structure of example.com without any network access.
        let html = r#"<!doctype html>
<html>
<head><title>Example Domain</title></head>
<body>
<div>
    <h1>Example Domain</h1>
    <p>This domain is for use in illustrative examples.</p>
    <p><a href="https://example.com">More information...</a></p>
</div>
</body>
</html>"#;
        let text = html_to_text(html);
        assert!(text.contains("Example Domain"));
        assert!(text.contains("illustrative examples"));
        assert!(!text.contains("<h1>"));
        assert!(!text.contains("<p>"));
    }

    #[test]
    fn test_html_to_text_collapses_whitespace() {
        let html = "<div>  one  </div><p>  two  </p><br>three";
        let text = html_to_text(html);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.contains(&"one"));
        assert!(lines.contains(&"two"));
        assert!(lines.contains(&"three"));
    }

    #[tokio::test]
    async fn test_code_interpreter_language_validation() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"code": "print('hello')", "language": "unsupported"});
        let result = execute_tool(
            "developer__code_interpreter",
            &args,
            PathBuf::from(".").as_path(),
            &security,
        )
        .await;
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unsupported language"));
    }

    #[tokio::test]
    async fn test_database_query_tool_requires_db() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"query": "SELECT 1 AS num"});
        let result = execute_tool(
            "developer__database_query",
            &args,
            PathBuf::from(".").as_path(),
            &security,
        )
        .await;
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("database not found"));
    }

    #[tokio::test]
    async fn test_send_network_request_reads_local_http_response() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 1024];
            let _ = socket.read(&mut buffer).await;
            let body = "hello from local server";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });

        let result = send_network_request(
            &format!("http://{addr}/"),
            "GET",
            Vec::new(),
            None,
            Some("direct"),
        )
        .await
        .unwrap();
        assert!(result.contains("status: 200 OK"));
        assert!(result.contains("hello from local server"));
    }
}
