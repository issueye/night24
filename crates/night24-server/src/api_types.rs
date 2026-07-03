use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[allow(dead_code)]
pub(crate) struct ReplyRequest {
    #[schema(example = "hello world")]
    #[serde(alias = "message")]
    pub(crate) text: String,
    #[schema(example = "echo")]
    pub(crate) provider: Option<String>,
    #[schema(example = "sk-...")]
    pub(crate) api_key: Option<String>,
    #[schema(example = "https://api.openai.com/v1")]
    pub(crate) base_url: Option<String>,
    #[schema(example = "gpt-4o-mini")]
    pub(crate) model: Option<String>,
    #[schema(example = "session-123")]
    pub(crate) session_id: Option<String>,
    #[schema(example = "allow_all")]
    pub(crate) permission_mode: Option<String>,
    #[schema(example = "http://127.0.0.1:7890")]
    pub(crate) network_proxy: Option<String>,
    #[schema(example = 24000)]
    pub(crate) context_threshold_tokens: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub(crate) struct OpenWorkspaceRequest {
    #[schema(example = "E:\\code\\issueye\\ai_agent\\night24")]
    pub(crate) path: String,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub(crate) struct WorkspacePathQuery {
    #[serde(default)]
    pub(crate) path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub(crate) struct WorkspaceDiffQuery {
    #[serde(default)]
    pub(crate) staged: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub(crate) struct CancelAgentRequest {
    #[schema(example = "run-123")]
    pub(crate) run_id: Option<String>,
    #[schema(example = "user_cancelled")]
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub(crate) struct PermissionDecisionRequest {
    #[schema(example = "run-123")]
    pub(crate) run_id: Option<String>,
    #[schema(example = "user denied running this command")]
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub(crate) struct CreateSessionRequest {
    #[schema(example = "my-chat")]
    pub(crate) name: Option<String>,
    #[schema(example = "user")]
    pub(crate) session_type: Option<String>,
    #[schema(example = "E:\\code\\project")]
    pub(crate) working_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub(crate) struct RenameSessionRequest {
    #[schema(example = "debugging rust")]
    pub(crate) name: String,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub(crate) struct ForkSessionRequest {
    /// Optional index at which to fork. If omitted, the full history is copied.
    #[schema(example = 4)]
    pub(crate) at_index: Option<usize>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct SessionSummary {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) session_type: String,
    pub(crate) working_dir: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct ReadyResponse {
    pub(crate) ready: bool,
    pub(crate) server: String,
    pub(crate) core: CoreStatusResponse,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct CoreStatusResponse {
    pub(crate) available: bool,
    pub(crate) initialized: bool,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct WorkspaceInfo {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) root_path: String,
    pub(crate) created_at: String,
    pub(crate) last_opened_at: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct RecentWorkspacesResponse {
    pub(crate) workspaces: Vec<WorkspaceInfo>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct WorkspaceTreeResponse {
    pub(crate) workspace: WorkspaceInfo,
    pub(crate) root: FileTreeNode,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct FileTreeNode {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) kind: String,
    pub(crate) children: Option<Vec<FileTreeNode>>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct WorkspaceFileResponse {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) is_binary: bool,
    pub(crate) content: Option<String>,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct WorkspaceStatusResponse {
    pub(crate) workspace: WorkspaceInfo,
    pub(crate) is_git_repo: bool,
    pub(crate) branch: Option<String>,
    pub(crate) files: Vec<WorkspaceStatusFile>,
    pub(crate) has_changes: bool,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct WorkspaceStatusFile {
    pub(crate) path: String,
    pub(crate) index_status: String,
    pub(crate) worktree_status: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct WorkspaceDiffResponse {
    pub(crate) workspace: WorkspaceInfo,
    pub(crate) staged: bool,
    pub(crate) diff: String,
    pub(crate) files_changed: usize,
    pub(crate) insertions: usize,
    pub(crate) deletions: usize,
    pub(crate) has_changes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceChangeSnapshot {
    pub(crate) status: String,
    pub(crate) diff: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct ToolsResponse {
    pub(crate) tools: Vec<night24_core::model::Tool>,
    pub(crate) source: String,
    pub(crate) core_available: bool,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct AcceptedResponse {
    pub(crate) accepted: bool,
    pub(crate) reason: Option<String>,
    pub(crate) run_id: Option<String>,
    pub(crate) permission_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceState {
    pub(crate) current: Option<WorkspaceInfo>,
    pub(crate) recent: Vec<WorkspaceInfo>,
}

impl WorkspaceState {
    pub(crate) fn new() -> Self {
        Self {
            current: None,
            recent: Vec::new(),
        }
    }
}
