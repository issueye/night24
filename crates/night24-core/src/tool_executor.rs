use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::{Column, Row};
use tokio::time::timeout;

use crate::model::Tool;
use crate::security::SecurityInspector;
use crate::permission::PermissionLevel;

pub fn builtin_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "developer__shell".to_string(),
            description: "Run a shell command in the working directory.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute."
                    }
                },
                "required": ["command"]
            }),
        },
        Tool {
            name: "developer__read_file".to_string(),
            description: "Read the content of a file within the working directory.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file to read."
                    }
                },
                "required": ["path"]
            }),
        },
        Tool {
            name: "developer__write_file".to_string(),
            description: "Write content to a file within the working directory.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file to write."
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write into the file."
                    }
                },
                "required": ["path", "content"]
            }),
        },
        Tool {
            name: "developer__echo".to_string(),
            description: "Echo back the provided text.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text to echo back."
                    }
                },
                "required": ["text"]
            }),
        },
        Tool {
            name: "developer__list_files".to_string(),
            description: "List files and directories under the given relative path.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative directory path to list. Defaults to working directory."
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "Whether to list recursively.",
                        "default": false
                    }
                },
                "required": []
            }),
        },
        Tool {
            name: "developer__datetime".to_string(),
            description: "Get the current UTC and local datetime.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "description": "Optional output format. Defaults to rfc3339.",
                        "default": "rfc3339"
                    }
                },
                "required": []
            }),
        },
        Tool {
            name: "developer__calculator".to_string(),
            description: "Evaluate a basic math expression safely.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "Math expression to evaluate, e.g. 2 + 3 * 4."
                    }
                },
                "required": ["expression"]
            }),
        },
        Tool {
            name: "developer__http_request".to_string(),
            description: "Make an HTTP request and return the response body.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Target URL."
                    },
                    "method": {
                        "type": "string",
                        "description": "HTTP method.",
                        "default": "GET"
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional JSON object of headers."
                    },
                    "body": {
                        "type": "string",
                        "description": "Optional request body."
                    }
                },
                "required": ["url"]
            }),
        },
        Tool {
            name: "developer__web_search".to_string(),
            description: "Search the web and return a short result summary.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query."
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "developer__jq".to_string(),
            description: "Query JSON data with a simple jq-like expression.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "data": {
                        "type": "string",
                        "description": "JSON string to query."
                    },
                    "query": {
                        "type": "string",
                        "description": "jq-like query expression."
                    }
                },
                "required": ["data", "query"]
            }),
        },
        Tool {
            name: "developer__file_search".to_string(),
            description: "Search files in the working directory for a text pattern.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Text pattern to search for."
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in. Defaults to working directory."
                    },
                    "file_pattern": {
                        "type": "string",
                        "description": "Optional glob pattern for filenames, e.g. *.rs."
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "developer__web_scraper".to_string(),
            description: "Fetch a web page and return extracted plain text.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Target URL to scrape."
                    }
                },
                "required": ["url"]
            }),
        },
        Tool {
            name: "developer__code_interpreter".to_string(),
            description: "Run a short Python or JavaScript snippet and return stdout.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "Code snippet to execute."
                    },
                    "language": {
                        "type": "string",
                        "description": "Language: python or javascript.",
                        "default": "python"
                    }
                },
                "required": ["code"]
            }),
        },
        Tool {
            name: "developer__database_query".to_string(),
            description: "Run a read-only SQL query against the local SQLite database.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "SQL query to execute."
                    }
                },
                "required": ["query"]
            }),
        },
    ]
}

async fn run_shell_command(command: &str, working_dir: &Path) -> anyhow::Result<String> {
    let working_dir = working_dir.to_path_buf();

    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("cmd");
    #[cfg(target_os = "windows")]
    cmd.args(["/C", command]);

    #[cfg(not(target_os = "windows"))]
    let mut cmd = std::process::Command::new("sh");
    #[cfg(not(target_os = "windows"))]
    cmd.args(["-c", command]);

    let output = tokio::task::spawn_blocking(move || cmd.current_dir(&working_dir).output())
        .await??;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        if stdout.is_empty() && stderr.is_empty() {
            Ok("(command executed with no output)".to_string())
        } else if stdout.is_empty() {
            Ok(stderr)
        } else {
            Ok(stdout)
        }
    } else {
        anyhow::bail!("shell command failed: {}", stderr);
    }
}

fn should_skip_dir(path: &std::path::Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        matches!(name, "target" | ".git" | "node_modules" | ".venv" | "venv" | "__pycache__")
    } else {
        false
    }
}

fn glob_match(pattern: &str, name: &str) -> bool {
    let mut pattern_chars = pattern.chars().peekable();
    let mut text_chars = name.chars().peekable();

    while let Some(&p) = pattern_chars.peek() {
        match p {
            '*' => {
                pattern_chars.next();
                while text_chars.peek().is_some() {
                    if glob_match(pattern_chars.clone().collect::<String>().as_str(), text_chars.clone().collect::<String>().as_str()) {
                        return true;
                    }
                    text_chars.next();
                }
                return pattern_chars.clone().collect::<String>().is_empty();
            }
            '?' => {
                pattern_chars.next();
                if text_chars.next().is_none() {
                    return false;
                }
            }
            _ => {
                if text_chars.next() != Some(p) {
                    return false;
                }
                pattern_chars.next();
            }
        }
    }

    text_chars.peek().is_none()
}

fn resolve_within_workdir(working_dir: &Path, user_path: &str) -> anyhow::Result<PathBuf> {
    let candidate = if Path::new(user_path).is_absolute() {
        PathBuf::from(user_path)
    } else {
        working_dir.join(user_path)
    };

    let canonical_workdir = working_dir.canonicalize().unwrap_or_else(|_| working_dir.to_path_buf());

    // For non-existent files, canonicalize the parent directory instead.
    let canonical_candidate = candidate.canonicalize().unwrap_or_else(|_| {
        let parent = candidate.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
        let canonical_parent = parent.canonicalize().unwrap_or_else(|_| parent.clone());
        canonical_parent.join(candidate.file_name().unwrap_or_default())
    });

    if !canonical_candidate.starts_with(&canonical_workdir) {
        anyhow::bail!("path escapes working directory: {}", user_path);
    }

    Ok(candidate)
}

/// Convert a raw HTML string into plain text by stripping tags and collapsing
/// whitespace. This is a pure function so it can be unit-tested without any
/// network access.
fn html_to_text(html: &str) -> String {
    let text = html
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<p>", "\n")
        .replace("</p>", "\n")
        .replace("<div>", "\n")
        .replace("</div>", "\n");

    let mut cleaned = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
            cleaned.push('\n');
        } else if !in_tag {
            cleaned.push(ch);
        }
    }

    let lines: Vec<&str> = cleaned
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    lines.join("\n")
}

pub async fn execute_tool(
    name: &str,
    arguments: &serde_json::Value,
    working_dir: &Path,
    security: &SecurityInspector,
) -> anyhow::Result<String> {
    timeout(Duration::from_secs(10), async {
        match name {
            "developer__echo" => {
                let text = arguments
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
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

                let findings = security.inspect_output(&content).await;
                if !findings.is_empty() {
                    return Ok(format!("security inspection: {}", findings.join("; ")));
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
                    .ok_or_else(|| anyhow::anyhow!("missing `content` for developer__write_file"))?;

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
                    .ok_or_else(|| anyhow::anyhow!("missing `expression` for developer__calculator"))?;

                let sanitized = expression
                    .chars()
                    .filter(|c| c.is_ascii_digit() || "+-*/().".contains(*c) || c.is_whitespace())
                    .collect::<String>();

                let value = meval::eval_str(&sanitized)
                    .map_err(|e| anyhow::anyhow!("calculator error: {}", e))?;
                Ok(format!("{}", value))
            }
            "developer__http_request" => {
                let url = arguments
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing `url` for developer__http_request"))?;

                let method = arguments
                    .get("method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("GET")
                    .to_uppercase();

                let headers = arguments
                    .get("headers")
                    .and_then(|v| v.as_object())
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                let body = arguments
                    .get("body")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let client = reqwest::Client::new();
                let mut request = match method.as_str() {
                    "GET" => client.get(url),
                    "POST" => client.post(url),
                    "PUT" => client.put(url),
                    "PATCH" => client.patch(url),
                    "DELETE" => client.delete(url),
                    "HEAD" => client.head(url),
                    _ => anyhow::bail!("unsupported http method: {}", method),
                };

                for (key, value) in headers {
                    request = request.header(&key, value);
                }

                if let Some(body) = body {
                    request = request.body(body);
                }

                let response = request.send().await?;
                let status = response.status();
                let text = response.text().await?;

                if status.is_success() {
                    Ok(text)
                } else {
                    anyhow::bail!("http request failed {}: {}", status, text);
                }
            }
            "developer__web_search" => {
                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing `query` for developer__web_search"))?;

                // Placeholder search result.
                Ok(format!(
                    "Simulated search results for: {}\n- Result 1: https://example.com/1\n- Result 2: https://example.com/2",
                    query
                ))
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

                Ok(format!(
                    "(unsupported jq-like query in Phase 1: {})",
                    query
                ))
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

                let file_pattern = arguments
                    .get("file_pattern")
                    .and_then(|v| v.as_str());

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
                                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
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

                let client = reqwest::Client::new();
                let response = client.get(url).send().await?;
                let html = response.text().await?;

                Ok(html_to_text(&html))
            }
            "developer__code_interpreter" => {
                let code = arguments
                    .get("code")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing `code` for developer__code_interpreter"))?;

                let language = arguments
                    .get("language")
                    .and_then(|v| v.as_str())
                    .unwrap_or("python")
                    .to_lowercase();

                let escaped = code.replace("\\", "\\\\").replace("\"", "\\\"");
                let output = match language.as_str() {
                    "python" => {
                        run_shell_command(&format!("python -u -c \"{}\"", escaped), working_dir).await?
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
                    .ok_or_else(|| anyhow::anyhow!("missing `query` for developer__database_query"))?;

                let sanitized = query.trim().to_lowercase();
                if sanitized.starts_with("drop") || sanitized.starts_with("delete") || sanitized.starts_with("update") || sanitized.starts_with("insert") || sanitized.starts_with("alter") {
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
                Ok(serde_json::to_string_pretty(&serde_json::Value::Array(result))?)
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

    #[tokio::test]
    async fn test_echo_tool() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"text": "hello"});
        let result = execute_tool("developer__echo", &args, PathBuf::from(".").as_path(), &security).await;
        assert_eq!(result.unwrap(), "hello");
    }

    #[tokio::test]
    async fn test_datetime_tool() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({});
        let result = execute_tool("developer__datetime", &args, PathBuf::from(".").as_path(), &security).await;
        assert!(result.unwrap().starts_with("utc=20"));
    }

    #[tokio::test]
    async fn test_calculator_tool() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"expression": "2 + 3 * 4"});
        let result = execute_tool("developer__calculator", &args, PathBuf::from(".").as_path(), &security).await;
        assert_eq!(result.unwrap(), "14");
    }

    #[tokio::test]
    async fn test_jq_tool() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"data": "{\"a\":1,\"b\":2}", "query": "keys"});
        let result = execute_tool("developer__jq", &args, PathBuf::from(".").as_path(), &security).await;
        assert!(result.unwrap().contains("a"));
    }

    #[tokio::test]
    async fn test_web_search_tool() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"query": "rust"});
        let result = execute_tool("developer__web_search", &args, PathBuf::from(".").as_path(), &security).await;
        assert!(result.unwrap().contains("rust"));
    }

    #[tokio::test]
    async fn test_shell_security_inspection() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"command": "rm -rf /"});
        let result = execute_tool("developer__shell", &args, PathBuf::from(".").as_path(), &security).await;
        assert!(result.unwrap().contains("security inspection"));
    }

    #[tokio::test]
    async fn test_unknown_tool() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({});
        let result = execute_tool("developer__unknown", &args, PathBuf::from(".").as_path(), &security).await;
        assert!(result.unwrap_err().to_string().contains("unknown tool"));
    }

    #[tokio::test]
    async fn test_file_search_tool() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"query": "developer__echo", "path": "."});
        let result = execute_tool("developer__file_search", &args, PathBuf::from(".").as_path(), &security).await;
        let output = result.unwrap();
        assert!(output.contains("tool_executor.rs"));
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
        let result = execute_tool("developer__code_interpreter", &args, PathBuf::from(".").as_path(), &security).await;
        assert!(result.unwrap_err().to_string().contains("unsupported language"));
    }

    #[tokio::test]
    async fn test_database_query_tool_requires_db() {
        let security = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let args = serde_json::json!({"query": "SELECT 1 AS num"});
        let result = execute_tool("developer__database_query", &args, PathBuf::from(".").as_path(), &security).await;
        assert!(result.unwrap_err().to_string().contains("database not found"));
    }
}
