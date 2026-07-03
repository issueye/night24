use std::collections::HashMap;
use std::sync::RwLock;

/// The decision returned by the permission system for a given tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    /// Tool execution is permitted without asking.
    Allow,
    /// Tool execution is rejected outright.
    Deny,
    /// Caller must obtain explicit confirmation before executing.
    Confirm,
}

/// Categorises tools so a single policy entry can cover a whole class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolCategory {
    Shell,
    Read,
    Write,
    Network,
    Other,
}

impl ToolCategory {
    /// Map a tool name (e.g. `developer__shell`) to its category.
    pub fn from_tool_name(name: &str) -> Self {
        match name {
            "developer__shell" | "developer__code_interpreter" => ToolCategory::Shell,
            "developer__read_file" | "developer__list_files" | "developer__file_search" => {
                ToolCategory::Read
            }
            "developer__write_file" => ToolCategory::Write,
            "developer__http_request"
            | "developer__network_request"
            | "developer__web_search"
            | "developer__network_search"
            | "developer__web_scraper" => ToolCategory::Network,
            _ => ToolCategory::Other,
        }
    }
}

/// Permission manager with an explicit per-tool / per-category policy table
/// plus a default fallback. Thread-safe via an `RwLock`.
pub struct PermissionManager {
    default_level: PermissionLevel,
    policies: RwLock<HashMap<String, PermissionLevel>>,
}

impl std::fmt::Debug for PermissionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PermissionManager")
            .field("default_level", &self.default_level)
            .field("policies_count", &self.policies.read().unwrap().len())
            .finish()
    }
}

impl Default for PermissionManager {
    fn default() -> Self {
        // Conservative default: everything requires confirmation. The caller
        // can relax this by setting explicit policies or changing the default.
        Self {
            default_level: PermissionLevel::Confirm,
            policies: RwLock::new(HashMap::new()),
        }
    }
}

impl PermissionManager {
    pub fn new(default_level: PermissionLevel) -> Self {
        Self {
            default_level,
            policies: RwLock::new(HashMap::new()),
        }
    }

    /// Read-only tools and echo are safe to auto-allow; everything else keeps
    /// the configured default. This is a convenience preset for local use.
    pub fn permissive_local() -> Self {
        let mut policies = HashMap::new();
        policies.insert("developer__echo".to_string(), PermissionLevel::Allow);
        policies.insert("developer__read_file".to_string(), PermissionLevel::Allow);
        policies.insert("developer__list_files".to_string(), PermissionLevel::Allow);
        policies.insert("developer__file_search".to_string(), PermissionLevel::Allow);
        policies.insert("developer__datetime".to_string(), PermissionLevel::Allow);
        policies.insert("developer__calculator".to_string(), PermissionLevel::Allow);
        policies.insert("developer__jq".to_string(), PermissionLevel::Allow);
        policies.insert(
            "developer__database_query".to_string(),
            PermissionLevel::Allow,
        );
        Self {
            default_level: PermissionLevel::Confirm,
            policies: RwLock::new(policies),
        }
    }

    /// Set an explicit policy for a tool name.
    pub fn set(&self, tool_name: impl Into<String>, level: PermissionLevel) {
        self.policies
            .write()
            .unwrap()
            .insert(tool_name.into(), level);
    }

    /// Resolve the permission level for a tool: exact-name policy first, then
    /// the default level.
    pub async fn check(&self, tool_name: &str) -> PermissionLevel {
        if let Some(level) = self.policies.read().unwrap().get(tool_name) {
            return *level;
        }
        self.default_level
    }

    /// Synchronous variant for contexts without async (e.g. tests).
    pub fn check_sync(&self, tool_name: &str) -> PermissionLevel {
        if let Some(level) = self.policies.read().unwrap().get(tool_name) {
            return *level;
        }
        self.default_level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_default_is_confirm() {
        let pm = PermissionManager::default();
        assert_eq!(pm.check("developer__shell").await, PermissionLevel::Confirm);
    }

    #[tokio::test]
    async fn test_explicit_policy_overrides_default() {
        let pm = PermissionManager::new(PermissionLevel::Confirm);
        pm.set("developer__read_file", PermissionLevel::Allow);
        assert_eq!(
            pm.check("developer__read_file").await,
            PermissionLevel::Allow
        );
        // Unmapped tool falls back to default.
        assert_eq!(pm.check("developer__other").await, PermissionLevel::Confirm);
    }

    #[tokio::test]
    async fn test_permissive_local_preset() {
        let pm = PermissionManager::permissive_local();
        // Read-only tools are allowed.
        assert_eq!(
            pm.check("developer__read_file").await,
            PermissionLevel::Allow
        );
        assert_eq!(pm.check("developer__echo").await, PermissionLevel::Allow);
        // Sensitive tools still require confirmation.
        assert_eq!(pm.check("developer__shell").await, PermissionLevel::Confirm);
        assert_eq!(
            pm.check("developer__write_file").await,
            PermissionLevel::Confirm
        );
    }

    #[test]
    fn test_tool_category_mapping() {
        assert_eq!(
            ToolCategory::from_tool_name("developer__shell"),
            ToolCategory::Shell
        );
        assert_eq!(
            ToolCategory::from_tool_name("developer__code_interpreter"),
            ToolCategory::Shell
        );
        assert_eq!(
            ToolCategory::from_tool_name("developer__read_file"),
            ToolCategory::Read
        );
        assert_eq!(
            ToolCategory::from_tool_name("developer__http_request"),
            ToolCategory::Network
        );
        assert_eq!(
            ToolCategory::from_tool_name("developer__network_search"),
            ToolCategory::Network
        );
        assert_eq!(
            ToolCategory::from_tool_name("developer__echo"),
            ToolCategory::Other
        );
    }
}
