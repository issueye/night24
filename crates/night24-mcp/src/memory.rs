//! Long-term memory store for Night24.
//!
//! A self-contained, thread-safe key/value memory that an agent can use to
//! persist facts across sessions. It is intentionally storage-agnostic: the
//! default backend is in-memory, but the same trait can be backed by SQLite or
//! a vector store later without changing call sites.

use std::collections::HashMap;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

/// A single stored memory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub key: String,
    pub value: String,
    pub tags: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Trait abstracting memory persistence. The default in-memory implementation
/// is sufficient for a single-process agent; future backends (SQLite, vector
/// DB) implement the same surface.
pub trait MemoryStore: Send + Sync {
    fn store(&self, key: &str, value: &str, tags: &[String]) -> anyhow::Result<()>;
    fn recall(&self, key: &str) -> anyhow::Result<Option<MemoryEntry>>;
    fn list(&self) -> anyhow::Result<Vec<MemoryEntry>>;
    fn search(&self, query: &str) -> anyhow::Result<Vec<MemoryEntry>>;
    fn clear(&self) -> anyhow::Result<usize>;
}

/// In-memory implementation of [`MemoryStore`], backed by a `RwLock<HashMap>`.
pub struct InMemoryStore {
    entries: RwLock<HashMap<String, MemoryEntry>>,
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MemoryStore for InMemoryStore {
    fn store(&self, key: &str, value: &str, tags: &[String]) -> anyhow::Result<()> {
        let entry = MemoryEntry {
            key: key.to_string(),
            value: value.to_string(),
            tags: tags.to_vec(),
            created_at: chrono::Utc::now(),
        };
        self.entries.write().unwrap().insert(key.to_string(), entry);
        Ok(())
    }

    fn recall(&self, key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        Ok(self.entries.read().unwrap().get(key).cloned())
    }

    fn list(&self) -> anyhow::Result<Vec<MemoryEntry>> {
        let mut entries: Vec<MemoryEntry> =
            self.entries.read().unwrap().values().cloned().collect();
        // Stable ordering by created_at then key.
        entries.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.key.cmp(&b.key))
        });
        Ok(entries)
    }

    fn search(&self, query: &str) -> anyhow::Result<Vec<MemoryEntry>> {
        let q = query.to_lowercase();
        let mut matches: Vec<MemoryEntry> = self
            .entries
            .read()
            .unwrap()
            .values()
            .filter(|e| {
                e.key.to_lowercase().contains(&q)
                    || e.value.to_lowercase().contains(&q)
                    || e.tags.iter().any(|t| t.to_lowercase().contains(&q))
            })
            .cloned()
            .collect();
        matches.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(matches)
    }

    fn clear(&self) -> anyhow::Result<usize> {
        let count = self.entries.read().unwrap().len();
        self.entries.write().unwrap().clear();
        Ok(count)
    }
}

/// JSON-schema descriptions for the memory tools, in the same shape as the
/// `developer__*` tools so they can be merged into the agent's tool list.
pub fn memory_tool_definitions() -> Vec<night24_core::model::Tool> {
    use night24_core::model::Tool;
    vec![
        Tool {
            name: "memory__store".to_string(),
            description: "Persist a fact under a key for later recall.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Unique key for the memory." },
                    "value": { "type": "string", "description": "Value to remember." },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional tags for grouping/search."
                    }
                },
                "required": ["key", "value"]
            }),
        },
        Tool {
            name: "memory__recall".to_string(),
            description: "Retrieve a previously stored memory by key.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Key of the memory to recall." }
                },
                "required": ["key"]
            }),
        },
        Tool {
            name: "memory__search".to_string(),
            description: "Search stored memories by a text query across keys, values, and tags.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Text to search for." }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "memory__clear".to_string(),
            description: "Delete all stored memories.".to_string(),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
        },
    ]
}

/// Execute a memory tool call against the given store. Returns the human-readable
/// result string, mirroring the contract of `tool_executor::execute_tool`.
pub fn execute_memory_tool(
    store: &dyn MemoryStore,
    name: &str,
    arguments: &serde_json::Value,
) -> anyhow::Result<String> {
    match name {
        "memory__store" => {
            let key = arguments
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing `key` for memory__store"))?;
            let value = arguments
                .get("value")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing `value` for memory__store"))?;
            let tags = arguments
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            store.store(key, value, &tags)?;
            Ok(format!("stored memory under key '{}'", key))
        }
        "memory__recall" => {
            let key = arguments
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing `key` for memory__recall"))?;
            match store.recall(key)? {
                Some(entry) => {
                    let tags = if entry.tags.is_empty() {
                        String::new()
                    } else {
                        format!(" [tags: {}]", entry.tags.join(", "))
                    };
                    Ok(format!("{}: {}{}", entry.key, entry.value, tags))
                }
                None => Ok(format!("(no memory for key '{}')", key)),
            }
        }
        "memory__search" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing `query` for memory__search"))?;
            let matches = store.search(query)?;
            if matches.is_empty() {
                Ok("(no matching memories)".to_string())
            } else {
                let lines: Vec<String> = matches
                    .iter()
                    .map(|e| format!("- {}: {}", e.key, e.value))
                    .collect();
                Ok(lines.join("\n"))
            }
        }
        "memory__clear" => {
            let n = store.clear()?;
            Ok(format!("cleared {} memories", n))
        }
        _ => anyhow::bail!("unknown memory tool: {}", name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_recall() {
        let store = InMemoryStore::new();
        store
            .store("lang", "rust", &["pl".to_string()])
            .unwrap();
        let entry = store.recall("lang").unwrap().unwrap();
        assert_eq!(entry.value, "rust");
        assert_eq!(entry.tags, vec!["pl".to_string()]);
    }

    #[test]
    fn test_recall_missing_key() {
        let store = InMemoryStore::new();
        assert!(store.recall("nope").unwrap().is_none());
    }

    #[test]
    fn test_search_by_value_and_tag() {
        let store = InMemoryStore::new();
        store.store("a", "rust language", &[]).unwrap();
        store.store("b", "python", &["pl".to_string()]).unwrap();
        let rust_matches = store.search("rust").unwrap();
        assert_eq!(rust_matches.len(), 1);
        let pl_matches = store.search("pl").unwrap();
        assert_eq!(pl_matches.len(), 1);
        assert_eq!(pl_matches[0].key, "b");
    }

    #[test]
    fn test_list_sorted_by_created_time() {
        let store = InMemoryStore::new();
        store.store("b", "2", &[]).unwrap();
        // A tiny delay ensures "a" has a strictly later created_at than "b".
        std::thread::sleep(std::time::Duration::from_millis(2));
        store.store("a", "1", &[]).unwrap();
        let list = store.list().unwrap();
        // Both entries present, oldest first ("b" was stored first).
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].key, "b");
        assert_eq!(list[1].key, "a");
    }

    #[test]
    fn test_clear() {
        let store = InMemoryStore::new();
        store.store("a", "1", &[]).unwrap();
        store.store("b", "2", &[]).unwrap();
        assert_eq!(store.clear().unwrap(), 2);
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn test_execute_memory_tool_store_and_recall() {
        let store = InMemoryStore::new();
        let args = serde_json::json!({"key": "k", "value": "v"});
        let result = execute_memory_tool(&store, "memory__store", &args).unwrap();
        assert!(result.contains("stored memory under key 'k'"));

        let recall_args = serde_json::json!({"key": "k"});
        let result = execute_memory_tool(&store, "memory__recall", &recall_args).unwrap();
        assert_eq!(result, "k: v");
    }

    #[test]
    fn test_execute_memory_tool_unknown() {
        let store = InMemoryStore::new();
        let err = execute_memory_tool(&store, "memory__bogus", &serde_json::json!({}));
        assert!(err.unwrap_err().to_string().contains("unknown memory tool"));
    }

    #[test]
    fn test_memory_tool_definitions_count() {
        let tools = memory_tool_definitions();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"memory__store"));
        assert!(names.contains(&"memory__recall"));
        assert!(names.contains(&"memory__search"));
        assert!(names.contains(&"memory__clear"));
    }
}
