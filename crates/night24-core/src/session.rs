use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::session_store::SessionStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionType {
    User,
    Scheduled,
    SubAgent,
    Hidden,
    Terminal,
    Gateway,
    Acp,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub session_type: SessionType,
    #[schema(value_type = String)]
    pub working_dir: PathBuf,
    pub conversation: Vec<crate::model::Message>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

impl Session {
    pub fn new(name: impl Into<String>, working_dir: PathBuf, session_type: SessionType) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            session_type,
            working_dir,
            conversation: Vec::new(),
            created_at: now,
            updated_at: now,
            archived_at: None,
        }
    }

    /// Create a new session that inherits this session's metadata and a
    /// truncated copy of its conversation. The new session gets a fresh id
    /// and timestamps, and a name derived from the original.
    ///
    /// If `at_index` is provided, only messages up to (excluding) that index
    /// are copied — enabling a fork from an arbitrary point in history.
    pub fn fork(&self, at_index: Option<usize>) -> Self {
        let now = Utc::now();
        let conversation = match at_index {
            Some(n) => self.conversation.iter().take(n).cloned().collect(),
            None => self.conversation.clone(),
        };
        let short_id = &self.id[..self.id.len().min(8)];
        Self {
            id: Uuid::new_v4().to_string(),
            name: format!("fork of {} ({})", self.name, short_id),
            session_type: self.session_type,
            working_dir: self.working_dir.clone(),
            conversation,
            created_at: now,
            updated_at: now,
            archived_at: None,
        }
    }

    /// Derive a concise, human-readable name from the first user message.
    /// Falls back to `session-{short_id}` when there is no usable text.
    pub fn derived_name(&self) -> String {
        for msg in &self.conversation {
            if msg.role != crate::model::Role::User {
                continue;
            }
            for block in &msg.content {
                if let crate::model::ContentBlock::Text { text } = block {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let single = trimmed
                        .chars()
                        .flat_map(|c| c.to_lowercase())
                        .collect::<String>();
                    let oneline = single.split_whitespace().collect::<Vec<_>>().join(" ");
                    let title: String = oneline.chars().take(48).collect();
                    return if oneline.chars().count() > 48 {
                        format!("{}…", title)
                    } else {
                        title
                    };
                }
            }
        }
        let short_id = &self.id[..self.id.len().min(8)];
        format!("session-{}", short_id)
    }

    /// Rename the session and refresh `updated_at`.
    pub fn rename(&mut self, new_name: impl Into<String>) {
        self.name = new_name.into();
        self.updated_at = Utc::now();
    }
}

pub struct SessionManager {
    store: Option<SessionStore>,
    memory: Arc<RwLock<HashMap<String, Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            store: None,
            memory: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn with_sqlite(database_url: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let store = SessionStore::new(
            database_url
                .as_ref()
                .to_str()
                .unwrap_or("sqlite:night24.db"),
        )
        .await?;
        Ok(Self {
            store: Some(store),
            memory: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn create(
        &self,
        name: impl Into<String>,
        working_dir: PathBuf,
        session_type: SessionType,
    ) -> anyhow::Result<Session> {
        if let Some(store) = &self.store {
            store.create_session(name, working_dir, session_type).await
        } else {
            let session = Session::new(name, working_dir, session_type);
            self.memory
                .write()
                .await
                .insert(session.id.clone(), session.clone());
            Ok(session)
        }
    }

    pub async fn get(&self, id: &str) -> anyhow::Result<Option<Session>> {
        if let Some(store) = &self.store {
            store.get_session(id).await
        } else {
            Ok(self.memory.read().await.get(id).cloned())
        }
    }

    pub async fn list(&self) -> anyhow::Result<Vec<Session>> {
        if let Some(store) = &self.store {
            store.list_sessions().await
        } else {
            Ok(self.memory.read().await.values().cloned().collect())
        }
    }

    pub async fn delete(&self, id: &str) -> anyhow::Result<bool> {
        if let Some(store) = &self.store {
            store.delete_session(id).await
        } else {
            Ok(self.memory.write().await.remove(id).is_some())
        }
    }

    pub async fn save(&self, session: &Session) -> anyhow::Result<()> {
        if let Some(store) = &self.store {
            store.upsert_session(session).await
        } else {
            self.memory
                .write()
                .await
                .insert(session.id.clone(), session.clone());
            Ok(())
        }
    }

    /// Fork an existing session. Returns the new session's id, or an error if
    /// the source session does not exist. `at_index` optionally limits the
    /// copied history to the first N messages.
    pub async fn fork(&self, source_id: &str, at_index: Option<usize>) -> anyhow::Result<Session> {
        let source = self
            .get(source_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("session not found: {}", source_id))?;
        let forked = source.fork(at_index);
        self.save(&forked).await?;
        Ok(forked)
    }

    /// Rename an existing session. Returns the updated session, or an error if
    /// the session does not exist.
    pub async fn rename(&self, id: &str, new_name: impl Into<String>) -> anyhow::Result<Session> {
        let mut session = self
            .get(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("session not found: {}", id))?;
        session.rename(new_name);
        self.save(&session).await?;
        Ok(session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let session = Session::new("test", PathBuf::from("/tmp"), SessionType::User);
        assert_eq!(session.name, "test");
        assert_eq!(session.working_dir, PathBuf::from("/tmp"));
        assert_eq!(session.session_type, SessionType::User);
        assert!(session.conversation.is_empty());
    }

    #[test]
    fn test_session_manager_in_memory() {
        let manager = SessionManager::new();
        assert!(manager.store.is_none());
    }

    #[tokio::test]
    async fn test_session_manager_create_without_store() {
        let manager = SessionManager::new();
        let session = manager
            .create("test", PathBuf::from("/tmp"), SessionType::User)
            .await;
        assert!(session.is_ok());
        assert_eq!(session.unwrap().name, "test");
    }

    #[test]
    fn test_session_fork_copies_history() {
        let mut session = Session::new("orig", PathBuf::from("/tmp"), SessionType::User);
        session
            .conversation
            .push(crate::model::Message::user("hello"));
        session
            .conversation
            .push(crate::model::Message::assistant("hi"));
        let forked = session.fork(None);
        assert_ne!(forked.id, session.id);
        assert_eq!(forked.conversation.len(), 2);
        assert!(forked.name.starts_with("fork of orig"));
    }

    #[test]
    fn test_session_fork_truncates_at_index() {
        let mut session = Session::new("orig", PathBuf::from("/tmp"), SessionType::User);
        session
            .conversation
            .push(crate::model::Message::user("one"));
        session
            .conversation
            .push(crate::model::Message::assistant("two"));
        session
            .conversation
            .push(crate::model::Message::user("three"));
        let forked = session.fork(Some(2));
        assert_eq!(forked.conversation.len(), 2);
        // Only the first two messages are kept.
        assert_eq!(forked.conversation[0].content.len(), 1);
    }

    #[test]
    fn test_session_derived_name_from_first_user_message() {
        let mut session = Session::new("orig", PathBuf::from("/tmp"), SessionType::User);
        session
            .conversation
            .push(crate::model::Message::user("  Help me debug Rust code  "));
        assert_eq!(session.derived_name(), "help me debug rust code");
    }

    #[test]
    fn test_session_derived_name_truncates_long_messages() {
        let mut session = Session::new("orig", PathBuf::from("/tmp"), SessionType::User);
        let long = "x".repeat(100);
        session.conversation.push(crate::model::Message::user(long));
        let name = session.derived_name();
        assert!(name.ends_with('…'));
        // 48 chars + ellipsis.
        assert_eq!(name.chars().count(), 49);
    }

    #[test]
    fn test_session_derived_name_fallback_when_empty() {
        let session = Session::new("orig", PathBuf::from("/tmp"), SessionType::User);
        let name = session.derived_name();
        assert!(name.starts_with("session-"));
    }

    #[test]
    fn test_session_rename() {
        let mut session = Session::new("orig", PathBuf::from("/tmp"), SessionType::User);
        session.rename("new name");
        assert_eq!(session.name, "new name");
    }

    #[tokio::test]
    async fn test_session_manager_fork_and_rename_in_memory() {
        let manager = SessionManager::new();
        let original = manager
            .create("orig", PathBuf::from("/tmp"), SessionType::User)
            .await
            .unwrap();
        manager
            .save(&{
                let mut s = original.clone();
                s.conversation.push(crate::model::Message::user("hello"));
                s
            })
            .await
            .unwrap();

        let forked = manager.fork(&original.id, None).await.unwrap();
        assert_ne!(forked.id, original.id);
        assert_eq!(forked.conversation.len(), 1);

        let renamed = manager.rename(&original.id, "renamed").await.unwrap();
        assert_eq!(renamed.name, "renamed");
    }

    #[tokio::test]
    async fn test_session_manager_sqlite_persists_between_instances() {
        let db_path =
            std::env::temp_dir().join(format!("night24-session-test-{}.db", uuid::Uuid::new_v4()));
        let db_url = format!(
            "sqlite:file:{}?mode=rwc",
            db_path.to_string_lossy().replace('\\', "/")
        );

        let manager = SessionManager::with_sqlite(&db_url).await.unwrap();
        let mut session = manager
            .create("persisted", PathBuf::from("."), SessionType::User)
            .await
            .unwrap();
        session
            .conversation
            .push(crate::model::Message::user("remember this"));
        manager.save(&session).await.unwrap();

        let reloaded_manager = SessionManager::with_sqlite(&db_url).await.unwrap();
        let reloaded = reloaded_manager
            .get(&session.id)
            .await
            .unwrap()
            .expect("session should persist");
        assert_eq!(reloaded.name, "persisted");
        assert_eq!(reloaded.conversation.len(), 1);

        let _ = std::fs::remove_file(db_path);
    }
}
