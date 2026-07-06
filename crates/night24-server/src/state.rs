use std::sync::Arc;

use night24_core::{provider::registry::ProviderRegistry, session::SessionManager};

use crate::agent_runner::{AgentRunner, RunnerMode};
use crate::api_types::WorkspaceState;
use crate::core_client::AgentCoreClient;
use crate::run_events::RunEventStore;

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct AppState {
    pub(crate) session_manager: Arc<SessionManager>,
    pub(crate) provider_registry: Arc<ProviderRegistry>,
    pub(crate) permission_manager: Arc<night24_core::permission::PermissionManager>,
    pub(crate) workspace_state: Arc<tokio::sync::RwLock<WorkspaceState>>,
    pub(crate) core_client: Arc<tokio::sync::RwLock<Option<Arc<AgentCoreClient>>>>,
    pub(crate) agent_runner: Arc<dyn AgentRunner>,
    pub(crate) runner_mode: RunnerMode,
    pub(crate) run_events: Arc<RunEventStore>,
}
