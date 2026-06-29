use rmcp::model::Tool;

#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub tool: Tool,
}

#[derive(Debug, Clone)]
pub struct ExtensionConfig {
    pub name: String,
    pub enabled: bool,
    pub command: Option<String>,
    pub args: Vec<String>,
}

pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn tools(&self) -> Vec<ToolInfo>;
}
