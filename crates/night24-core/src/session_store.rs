use anyhow::Context;
use chrono::{DateTime, Utc};
use serde_json;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use tracing::warn;

use std::collections::HashMap;
use std::convert::TryInto;

use crate::model::{ContentBlock, Message, Role};
use crate::session::{Session, SessionType};

#[derive(Debug, Clone)]
pub struct SessionStore {
    pool: Pool<Sqlite>,
}

impl SessionStore {
    pub async fn new(database_url: &str) -> anyhow::Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .with_context(|| format!("failed to connect to sqlite database: {database_url}"))?;

        let store = Self { pool };
        store.ensure_schema().await?;
        Ok(store)
    }

    async fn ensure_schema(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                session_type TEXT NOT NULL,
                working_dir TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                archived_at TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                session_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                role TEXT NOT NULL,
                message_type TEXT NOT NULL,
                content TEXT NOT NULL,
                text_content TEXT,
                tool_call_id TEXT,
                tool_name TEXT,
                tool_arguments TEXT,
                tool_output TEXT,
                tool_is_error INTEGER,
                tool_requests TEXT,
                tool_responses TEXT,
                created_at TEXT NOT NULL,
                PRIMARY KEY (session_id, message_id),
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_messages_session_position
            ON messages(session_id, position)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS tasks (
                session_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                completed INTEGER NOT NULL,
                source_message_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (session_id, task_id),
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_tasks_session_position
            ON tasks(session_id, position)
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn create_session(
        &self,
        name: impl Into<String>,
        working_dir: std::path::PathBuf,
        session_type: SessionType,
    ) -> anyhow::Result<Session> {
        let session = Session::new(name, working_dir, session_type);
        self.upsert_session(&session).await?;
        Ok(session)
    }

    pub async fn upsert_session(&self, session: &Session) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO sessions (id, name, session_type, working_dir, created_at, updated_at, archived_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                session_type = excluded.session_type,
                working_dir = excluded.working_dir,
                updated_at = excluded.updated_at,
                archived_at = excluded.archived_at
            "#,
        )
        .bind(&session.id)
        .bind(&session.name)
        .bind(session_type_to_str(session.session_type))
        .bind(session.working_dir.to_string_lossy().as_ref())
        .bind(session.created_at.to_rfc3339())
        .bind(session.updated_at.to_rfc3339())
        .bind(session.archived_at.map(|dt| dt.to_rfc3339()))
        .execute(&mut *tx)
        .await?;

        sqlx::query(r#"DELETE FROM messages WHERE session_id = ?1"#)
            .bind(&session.id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(r#"DELETE FROM tasks WHERE session_id = ?1"#)
            .bind(&session.id)
            .execute(&mut *tx)
            .await?;

        let mut tool_names_by_call_id = HashMap::new();
        for (position, message) in session.conversation.iter().enumerate() {
            insert_message(
                &mut tx,
                &session.id,
                position,
                message,
                &tool_names_by_call_id,
            )
            .await?;
            record_tool_request_names(message, &mut tool_names_by_call_id);
        }
        insert_session_tasks(&mut tx, session).await?;

        tx.commit().await?;

        Ok(())
    }

    pub async fn get_session(&self, id: &str) -> anyhow::Result<Option<Session>> {
        let row = sqlx::query_as::<_, SessionMetadataRow>(
            r#"SELECT id, name, session_type, working_dir, created_at, updated_at, archived_at FROM sessions WHERE id = ?1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let mut session: Session = row.try_into()?;
        session.conversation = self.load_messages(&session.id).await?;
        Ok(Some(session))
    }

    pub async fn list_sessions(&self) -> anyhow::Result<Vec<Session>> {
        let rows = sqlx::query_as::<_, SessionMetadataRow>(
            r#"SELECT id, name, session_type, working_dir, created_at, updated_at, archived_at FROM sessions ORDER BY updated_at DESC"#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut sessions = Vec::new();
        for row in rows {
            if let Ok(mut session) = TryInto::<Session>::try_into(row) {
                session.conversation = self.load_messages(&session.id).await?;
                sessions.push(session);
            }
        }
        Ok(sessions)
    }

    pub async fn delete_session(&self, id: &str) -> anyhow::Result<bool> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(r#"DELETE FROM messages WHERE session_id = ?1"#)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(r#"DELETE FROM tasks WHERE session_id = ?1"#)
            .bind(id)
            .execute(&mut *tx)
            .await?;

        let result = sqlx::query(r#"DELETE FROM sessions WHERE id = ?1"#)
            .bind(id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        Ok(result.rows_affected() > 0)
    }

    async fn load_messages(&self, session_id: &str) -> anyhow::Result<Vec<Message>> {
        let rows = sqlx::query_as::<_, MessageRow>(
            r#"
            SELECT message_id, role, content, created_at
            FROM messages
            WHERE session_id = ?1
            ORDER BY position ASC, created_at ASC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        let mut messages = Vec::new();
        for row in rows {
            match row.try_into() {
                Ok(message) => messages.push(message),
                Err(error) => warn!(
                    session_id,
                    error = ?error,
                    "skipping invalid persisted message"
                ),
            }
        }

        Ok(messages)
    }
}

#[derive(sqlx::FromRow)]
struct SessionMetadataRow {
    id: String,
    name: String,
    session_type: String,
    working_dir: String,
    created_at: String,
    updated_at: String,
    archived_at: Option<String>,
}

impl TryInto<Session> for SessionMetadataRow {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<Session, Self::Error> {
        let session_type = session_type_from_str(&self.session_type)?;

        Ok(Session {
            id: self.id,
            name: self.name,
            session_type,
            working_dir: std::path::PathBuf::from(self.working_dir),
            conversation: Vec::new(),
            created_at: DateTime::parse_from_rfc3339(&self.created_at)?.with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&self.updated_at)?.with_timezone(&Utc),
            archived_at: self.archived_at.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            }),
        })
    }
}

#[derive(sqlx::FromRow)]
struct MessageRow {
    message_id: String,
    role: String,
    content: String,
    created_at: String,
}

impl TryInto<Message> for MessageRow {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<Message, Self::Error> {
        Ok(Message {
            id: self.message_id,
            role: role_from_str(&self.role)?,
            content: serde_json::from_str(&self.content)?,
            created_at: DateTime::parse_from_rfc3339(&self.created_at)?.with_timezone(&Utc),
        })
    }
}

async fn insert_message(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    session_id: &str,
    position: usize,
    message: &Message,
    tool_names_by_call_id: &HashMap<String, String>,
) -> anyhow::Result<()> {
    let metadata = MessageMetadata::from_message(message, tool_names_by_call_id)?;
    let content = serde_json::to_string(&message.content)?;

    sqlx::query(
        r#"
        INSERT INTO messages (
            session_id,
            message_id,
            position,
            role,
            message_type,
            content,
            text_content,
            tool_call_id,
            tool_name,
            tool_arguments,
            tool_output,
            tool_is_error,
            tool_requests,
            tool_responses,
            created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        "#,
    )
    .bind(session_id)
    .bind(&message.id)
    .bind(position as i64)
    .bind(role_to_str(message.role))
    .bind(metadata.message_type)
    .bind(content)
    .bind(metadata.text_content)
    .bind(metadata.tool_call_id)
    .bind(metadata.tool_name)
    .bind(metadata.tool_arguments)
    .bind(metadata.tool_output)
    .bind(
        metadata
            .tool_is_error
            .map(|value| if value { 1_i64 } else { 0_i64 }),
    )
    .bind(metadata.tool_requests)
    .bind(metadata.tool_responses)
    .bind(message.created_at.to_rfc3339())
    .execute(&mut **tx)
    .await?;

    Ok(())
}

fn record_tool_request_names(
    message: &Message,
    tool_names_by_call_id: &mut HashMap<String, String>,
) {
    for block in &message.content {
        if let ContentBlock::ToolRequest { id, name, .. } = block {
            tool_names_by_call_id.insert(id.clone(), name.clone());
        }
    }
}

async fn insert_session_tasks(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    session: &Session,
) -> anyhow::Result<()> {
    let Some(task_list) = latest_task_list(&session.conversation) else {
        return Ok(());
    };

    for (position, task) in task_list.tasks.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO tasks (
                session_id,
                task_id,
                position,
                title,
                status,
                completed,
                source_message_id,
                created_at,
                updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
        )
        .bind(&session.id)
        .bind(task_id(position, &task.title))
        .bind(position as i64)
        .bind(&task.title)
        .bind(if task.completed {
            "completed"
        } else {
            "pending"
        })
        .bind(if task.completed { 1_i64 } else { 0_i64 })
        .bind(&task_list.source_message_id)
        .bind(task_list.updated_at.to_rfc3339())
        .bind(task_list.updated_at.to_rfc3339())
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

struct PersistedTaskList {
    source_message_id: String,
    updated_at: DateTime<Utc>,
    tasks: Vec<PersistedTask>,
}

struct PersistedTask {
    title: String,
    completed: bool,
}

fn latest_task_list(messages: &[Message]) -> Option<PersistedTaskList> {
    let mut latest = None;

    for message in messages {
        let text = task_text_from_message(message);
        let task_lists = extract_task_lists(&text);
        if let Some(tasks) = task_lists
            .into_iter()
            .last()
            .filter(|tasks| !tasks.is_empty())
        {
            latest = Some(PersistedTaskList {
                source_message_id: message.id.clone(),
                updated_at: message.created_at,
                tasks,
            });
        }
    }

    latest
}

fn task_text_from_message(message: &Message) -> String {
    match message.role {
        Role::Assistant | Role::Tool => message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                ContentBlock::ToolResponse { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

fn extract_task_lists(text: &str) -> Vec<Vec<PersistedTask>> {
    let mut lists = Vec::new();
    let mut active_lines: Option<Vec<&str>> = None;

    for line in text.lines() {
        if let Some(heading) = markdown_heading(line) {
            if let Some(lines) = active_lines.take() {
                let tasks = parse_tasks(&lines);
                if !tasks.is_empty() {
                    lists.push(tasks);
                }
            }
            active_lines = is_task_heading(heading).then(Vec::new);
            continue;
        }

        if let Some(lines) = active_lines.as_mut() {
            lines.push(line);
        }
    }

    if let Some(lines) = active_lines {
        let tasks = parse_tasks(&lines);
        if !tasks.is_empty() {
            lists.push(tasks);
        }
    }

    if lists.is_empty() {
        let lines: Vec<&str> = text.lines().collect();
        let tasks = parse_tasks(&lines);
        if !tasks.is_empty() {
            lists.push(tasks);
        }
    }

    lists
}

fn markdown_heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let heading = trimmed.trim_start_matches('#').trim();
    (!heading.is_empty()).then(|| heading.trim_end_matches('#').trim())
}

fn is_task_heading(value: &str) -> bool {
    matches!(
        value
            .trim_end_matches([':', '：'])
            .trim()
            .to_lowercase()
            .as_str(),
        "任务列表" | "任务清单" | "步骤任务" | "执行计划" | "task list" | "tasks" | "plan"
    )
}

fn parse_tasks(lines: &[&str]) -> Vec<PersistedTask> {
    lines
        .iter()
        .filter_map(|line| parse_task_line(line))
        .collect()
}

fn parse_task_line(line: &str) -> Option<PersistedTask> {
    let line = line.trim_start();
    let content = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
        .or_else(|| numbered_item_body(line))?;
    let content = content.trim_start();
    let marker_end = content.find(']')?;
    let marker = content.strip_prefix('[')?.get(..marker_end - 1)?;
    let title = content.get(marker_end + 1..)?.trim();
    if title.is_empty() {
        return None;
    }

    Some(PersistedTask {
        title: title.to_string(),
        completed: is_completed_marker(marker),
    })
}

fn numbered_item_body(line: &str) -> Option<&str> {
    let split = line.find(['.', ')'])?;
    if !line.get(..split)?.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    line.get(split + 1..).map(str::trim_start)
}

fn is_completed_marker(value: &str) -> bool {
    matches!(
        value.trim().to_lowercase().as_str(),
        "x" | "done" | "completed" | "complete" | "✓" | "✔" | "完成"
    )
}

fn task_id(position: usize, title: &str) -> String {
    let slug = title
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!(
        "task-{}-{}",
        position,
        if slug.is_empty() { "item" } else { &slug }
    )
}

struct MessageMetadata {
    message_type: &'static str,
    text_content: Option<String>,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    tool_arguments: Option<String>,
    tool_output: Option<String>,
    tool_is_error: Option<bool>,
    tool_requests: Option<String>,
    tool_responses: Option<String>,
}

impl MessageMetadata {
    fn from_message(
        message: &Message,
        tool_names_by_call_id: &HashMap<String, String>,
    ) -> anyhow::Result<Self> {
        let text_parts: Vec<String> = message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        let tool_requests: Vec<serde_json::Value> = message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolRequest {
                    id,
                    name,
                    arguments,
                } => Some(serde_json::json!({
                    "id": id,
                    "name": name,
                    "arguments": arguments,
                })),
                _ => None,
            })
            .collect();
        let tool_responses: Vec<serde_json::Value> = message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolResponse {
                    id,
                    content,
                    is_error,
                } => Some(serde_json::json!({
                    "id": id,
                    "content": content,
                    "is_error": is_error,
                })),
                _ => None,
            })
            .collect();

        let primary_request = tool_requests.first();
        let primary_response = tool_responses.first();
        let primary_response_tool_name = primary_response
            .and_then(|value| value.get("id"))
            .and_then(|value| value.as_str())
            .and_then(|id| tool_names_by_call_id.get(id))
            .cloned();

        Ok(Self {
            message_type: message_type(&message.content),
            text_content: (!text_parts.is_empty()).then(|| text_parts.join("\n")),
            tool_call_id: primary_request
                .and_then(|value| value.get("id"))
                .or_else(|| primary_response.and_then(|value| value.get("id")))
                .and_then(|value| value.as_str())
                .map(str::to_string),
            tool_name: primary_request
                .and_then(|value| value.get("name"))
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .or(primary_response_tool_name),
            tool_arguments: primary_request
                .and_then(|value| value.get("arguments"))
                .map(serde_json::to_string)
                .transpose()?,
            tool_output: primary_response
                .and_then(|value| value.get("content"))
                .and_then(|value| value.as_str())
                .map(str::to_string),
            tool_is_error: primary_response
                .and_then(|value| value.get("is_error"))
                .and_then(|value| value.as_bool()),
            tool_requests: (!tool_requests.is_empty())
                .then(|| serde_json::to_string(&tool_requests))
                .transpose()?,
            tool_responses: (!tool_responses.is_empty())
                .then(|| serde_json::to_string(&tool_responses))
                .transpose()?,
        })
    }
}

fn message_type(content: &[ContentBlock]) -> &'static str {
    let mut has_text = false;
    let mut has_tool_request = false;
    let mut has_tool_response = false;
    let mut has_thinking = false;

    for block in content {
        match block {
            ContentBlock::Text { .. } => has_text = true,
            ContentBlock::ToolRequest { .. } => has_tool_request = true,
            ContentBlock::ToolResponse { .. } => has_tool_response = true,
            ContentBlock::Thinking { .. } => has_thinking = true,
        }
    }

    match (has_text, has_tool_request, has_tool_response, has_thinking) {
        (false, false, false, false) => "empty",
        (true, false, false, false) => "text",
        (false, true, false, false) => "tool_request",
        (false, false, true, false) => "tool_response",
        (false, false, false, true) => "thinking",
        _ => "mixed",
    }
}

fn role_to_str(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::System => "system",
        Role::Tool => "tool",
    }
}

fn role_from_str(value: &str) -> anyhow::Result<Role> {
    match value {
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "system" => Ok(Role::System),
        "tool" => Ok(Role::Tool),
        other => anyhow::bail!("unknown message role: {}", other),
    }
}

fn session_type_to_str(st: SessionType) -> &'static str {
    match st {
        SessionType::User => "user",
        SessionType::Scheduled => "scheduled",
        SessionType::SubAgent => "sub_agent",
        SessionType::Hidden => "hidden",
        SessionType::Terminal => "terminal",
        SessionType::Gateway => "gateway",
        SessionType::Acp => "acp",
    }
}

fn session_type_from_str(s: &str) -> anyhow::Result<SessionType> {
    match s {
        "user" => Ok(SessionType::User),
        "scheduled" => Ok(SessionType::Scheduled),
        "sub_agent" => Ok(SessionType::SubAgent),
        "hidden" => Ok(SessionType::Hidden),
        "terminal" => Ok(SessionType::Terminal),
        "gateway" => Ok(SessionType::Gateway),
        "acp" => Ok(SessionType::Acp),
        other => anyhow::bail!("unknown session type: {}", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_url() -> (std::path::PathBuf, String) {
        let db_path = std::env::temp_dir().join(format!(
            "night24-session-store-test-{}.db",
            uuid::Uuid::new_v4()
        ));
        let db_url = format!(
            "sqlite:file:{}?mode=rwc",
            db_path.to_string_lossy().replace('\\', "/")
        );
        (db_path, db_url)
    }

    #[tokio::test]
    async fn new_schema_stores_messages_outside_sessions() {
        let (db_path, db_url) = temp_db_url();
        let store = SessionStore::new(&db_url).await.unwrap();
        let mut session = Session::new("split", std::path::PathBuf::from("."), SessionType::User);
        session.conversation.push(Message::user("hello"));
        session.conversation.push(Message::assistant("hi"));

        store.upsert_session(&session).await.unwrap();

        let reloaded = store
            .get_session(&session.id)
            .await
            .unwrap()
            .expect("session should reload");
        assert_eq!(reloaded.conversation.len(), 2);
        assert_eq!(reloaded.conversation[0].role, Role::User);

        let message_count: i64 =
            sqlx::query_scalar(r#"SELECT COUNT(*) FROM messages WHERE session_id = ?1"#)
                .bind(&session.id)
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(message_count, 2);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tool_request_metadata_is_saved_in_message_columns() {
        let (db_path, db_url) = temp_db_url();
        let store = SessionStore::new(&db_url).await.unwrap();
        let mut session = Session::new("tools", std::path::PathBuf::from("."), SessionType::User);
        session.conversation.push(Message {
            id: "tool-request-message".to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: "call-1".to_string(),
                name: "developer__read_file".to_string(),
                arguments: serde_json::json!({ "path": "README.md" }),
            }],
            created_at: Utc::now(),
        });

        store.upsert_session(&session).await.unwrap();

        let row: (String, String, String, String, String) = sqlx::query_as(
            r#"
            SELECT message_type, tool_call_id, tool_name, tool_arguments, tool_requests
            FROM messages
            WHERE message_id = 'tool-request-message'
            "#,
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(row.0, "tool_request");
        assert_eq!(row.1, "call-1");
        assert_eq!(row.2, "developer__read_file");
        assert_eq!(row.3, r#"{"path":"README.md"}"#);
        assert!(row.4.contains("developer__read_file"));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tool_response_metadata_is_saved_in_message_columns() {
        let (db_path, db_url) = temp_db_url();
        let store = SessionStore::new(&db_url).await.unwrap();
        let mut session = Session::new("tools", std::path::PathBuf::from("."), SessionType::User);
        session.conversation.push(Message {
            id: "tool-request-message".to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::ToolRequest {
                id: "call-1".to_string(),
                name: "developer__read_file".to_string(),
                arguments: serde_json::json!({ "path": "README.md" }),
            }],
            created_at: Utc::now(),
        });
        session
            .conversation
            .push(Message::tool_response("call-1", "file contents", true));

        store.upsert_session(&session).await.unwrap();

        let row: (String, String, String, String, i64, String) = sqlx::query_as(
            r#"
            SELECT message_type, tool_call_id, tool_name, tool_output, tool_is_error, tool_responses
            FROM messages
            WHERE session_id = ?1
              AND message_type = 'tool_response'
            "#,
        )
        .bind(&session.id)
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(row.0, "tool_response");
        assert_eq!(row.1, "call-1");
        assert_eq!(row.2, "developer__read_file");
        assert_eq!(row.3, "file contents");
        assert_eq!(row.4, 1);
        assert!(row.5.contains("file contents"));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn task_list_is_saved_in_tasks_table() {
        let (db_path, db_url) = temp_db_url();
        let store = SessionStore::new(&db_url).await.unwrap();
        let mut session = Session::new("tasks", std::path::PathBuf::from("."), SessionType::User);
        session.conversation.push(Message {
            id: "task-message-1".to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "## 任务列表\n- [x] 分析需求\n- [ ] 编写实现".to_string(),
            }],
            created_at: Utc::now(),
        });

        store.upsert_session(&session).await.unwrap();

        let rows: Vec<(String, String, i64, String)> = sqlx::query_as(
            r#"
            SELECT title, status, completed, source_message_id
            FROM tasks
            WHERE session_id = ?1
            ORDER BY position ASC
            "#,
        )
        .bind(&session.id)
        .fetch_all(&store.pool)
        .await
        .unwrap();

        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            (
                "分析需求".to_string(),
                "completed".to_string(),
                1,
                "task-message-1".to_string()
            )
        );
        assert_eq!(
            rows[1],
            (
                "编写实现".to_string(),
                "pending".to_string(),
                0,
                "task-message-1".to_string()
            )
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn deleting_session_removes_message_rows() {
        let (db_path, db_url) = temp_db_url();
        let store = SessionStore::new(&db_url).await.unwrap();
        let mut session = Session::new("delete", std::path::PathBuf::from("."), SessionType::User);
        session.conversation.push(Message {
            id: "delete-message-1".to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "bye".to_string(),
            }],
            created_at: Utc::now(),
        });
        session.conversation.push(Message {
            id: "delete-task-message".to_string(),
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "## 任务列表\n- [x] 清理任务".to_string(),
            }],
            created_at: Utc::now(),
        });
        store.upsert_session(&session).await.unwrap();

        assert!(store.delete_session(&session.id).await.unwrap());

        let message_count: i64 =
            sqlx::query_scalar(r#"SELECT COUNT(*) FROM messages WHERE session_id = ?1"#)
                .bind(&session.id)
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(message_count, 0);

        let task_count: i64 =
            sqlx::query_scalar(r#"SELECT COUNT(*) FROM tasks WHERE session_id = ?1"#)
                .bind(&session.id)
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(task_count, 0);

        let _ = std::fs::remove_file(db_path);
    }
}
