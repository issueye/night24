use std::sync::Arc;

use super::permission::{PermissionLevel, PermissionManager};

#[derive(Debug, Clone)]
pub struct SecurityInspector {
    pub permission_manager: Arc<PermissionManager>,
}

impl SecurityInspector {
    pub fn new(permission_manager: Arc<PermissionManager>) -> Self {
        Self { permission_manager }
    }

    pub async fn inspect_input(&self, text: &str) -> Vec<String> {
        let mut findings = Vec::new();

        let lower = text.to_lowercase();
        if lower.contains("rm -rf /") || lower.contains("rm -rf /*") {
            findings.push("Potential dangerous command: recursive delete of root".to_string());
        }
        if lower.contains("drop database") || lower.contains("drop schema") {
            findings.push("Potential destructive database command".to_string());
        }
        if lower.contains("curl") && (lower.contains("| sh") || lower.contains("| bash")) {
            findings.push("Potential remote code execution via pipe to shell".to_string());
        }
        if lower.contains("sudo") && (lower.contains("rm") || lower.contains("mv")) {
            findings.push("Potential privileged destructive command".to_string());
        }

        findings
    }

    pub async fn inspect_output(&self, text: &str) -> Vec<String> {
        let mut findings = Vec::new();

        let lower = text.to_lowercase();
        if lower.contains("password") || lower.contains("secret") || lower.contains("token") {
            if lower.contains("=") || lower.contains(":") {
                findings.push(
                    "Output may contain sensitive credential-like values".to_string(),
                );
            }
        }

        findings
    }

    pub async fn require_permission(&self, tool_name: &str) -> PermissionLevel {
        self.permission_manager.check(tool_name).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_inspect_input_detects_dangerous_rm() {
        let inspector = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let findings = inspector.inspect_input("rm -rf /").await;
        assert!(!findings.is_empty());
        assert!(findings[0].contains("recursive delete of root"));
    }

    #[tokio::test]
    async fn test_inspect_input_detects_drop_database() {
        let inspector = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let findings = inspector.inspect_input("drop database users").await;
        assert!(!findings.is_empty());
        assert!(findings[0].contains("destructive database command"));
    }

    #[tokio::test]
    async fn test_inspect_input_allows_safe_command() {
        let inspector = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let findings = inspector.inspect_input("echo hello").await;
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn test_inspect_output_detects_password() {
        let inspector = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let findings = inspector.inspect_output("password = secret123").await;
        assert!(!findings.is_empty());
        assert!(findings[0].contains("credential-like"));
    }

    #[tokio::test]
    async fn test_inspect_output_allows_safe_output() {
        let inspector = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let findings = inspector.inspect_output("hello world").await;
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn test_require_permission_default_confirm() {
        let inspector = SecurityInspector::new(std::sync::Arc::new(PermissionManager::default()));
        let level = inspector.require_permission("developer__shell").await;
        assert_eq!(level, PermissionLevel::Confirm);
    }
}
