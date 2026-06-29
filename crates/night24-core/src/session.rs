use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::session_store::SessionStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub session_type: SessionType,
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
        let store = SessionStore::new(database_url.as_ref().to_str().unwrap_or("sqlite:night24.db")).await?;
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
        let session = manager.create("test", PathBuf::from("/tmp"), SessionType::User).await;
        assert!(session.is_ok());
        assert_eq!(session.unwrap().name, "test");
    }
}
