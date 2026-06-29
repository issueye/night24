use anyhow::Context;
use chrono::{DateTime, Utc};
use serde_json;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};

use std::convert::TryInto;

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
                conversation TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                archived_at TEXT
            )
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
        let conversation = serde_json::to_string(&session.conversation)?;

        sqlx::query(
            r#"
            INSERT INTO sessions (id, name, session_type, working_dir, conversation, created_at, updated_at, archived_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                session_type = excluded.session_type,
                working_dir = excluded.working_dir,
                conversation = excluded.conversation,
                updated_at = excluded.updated_at,
                archived_at = excluded.archived_at
            "#,
        )
        .bind(&session.id)
        .bind(&session.name)
        .bind(session_type_to_str(session.session_type))
        .bind(session.working_dir.to_string_lossy().as_ref())
        .bind(conversation)
        .bind(session.created_at.to_rfc3339())
        .bind(session.updated_at.to_rfc3339())
        .bind(session.archived_at.map(|dt| dt.to_rfc3339()))
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_session(&self, id: &str) -> anyhow::Result<Option<Session>> {
        let row = sqlx::query_as::<_, SessionRow>(
            r#"SELECT id, name, session_type, working_dir, conversation, created_at, updated_at, archived_at FROM sessions WHERE id = ?1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.and_then(|r| r.try_into().ok()))
    }

    pub async fn list_sessions(&self) -> anyhow::Result<Vec<Session>> {
        let rows = sqlx::query_as::<_, SessionRow>(
            r#"SELECT id, name, session_type, working_dir, conversation, created_at, updated_at, archived_at FROM sessions ORDER BY updated_at DESC"#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut sessions = Vec::new();
        for row in rows {
            if let Ok(session) = row.try_into() {
                sessions.push(session);
            }
        }
        Ok(sessions)
    }

    pub async fn delete_session(&self, id: &str) -> anyhow::Result<bool> {
        let result = sqlx::query(r#"DELETE FROM sessions WHERE id = ?1"#)
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    id: String,
    name: String,
    session_type: String,
    working_dir: String,
    conversation: String,
    created_at: String,
    updated_at: String,
    archived_at: Option<String>,
}

impl TryInto<Session> for SessionRow {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<Session, Self::Error> {
        let conversation = serde_json::from_str(&self.conversation)?;
        let session_type = session_type_from_str(&self.session_type)?;

        Ok(Session {
            id: self.id,
            name: self.name,
            session_type,
            working_dir: std::path::PathBuf::from(self.working_dir),
            conversation,
            created_at: DateTime::parse_from_rfc3339(&self.created_at)?.with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&self.updated_at)?.with_timezone(&Utc),
            archived_at: self.archived_at.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))),
        })
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
