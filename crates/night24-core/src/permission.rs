#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    Allow,
    Deny,
    Confirm,
}

#[derive(Debug, Clone)]
pub struct PermissionManager {
    pub default_level: PermissionLevel,
}

impl Default for PermissionManager {
    fn default() -> Self {
        Self {
            default_level: PermissionLevel::Confirm,
        }
    }
}

impl PermissionManager {
    pub fn new(default_level: PermissionLevel) -> Self {
        Self { default_level }
    }

    pub async fn check(&self, _tool_name: &str) -> PermissionLevel {
        self.default_level
    }
}
