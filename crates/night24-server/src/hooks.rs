use std::path::{Path, PathBuf};

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::api_types::WorkspaceInfo;
use crate::workspace::current_workspace_info;
use crate::{json_error, AppState};

const HOOKS_DIR: &str = ".night24";
const HOOKS_FILE: &str = "hooks.json";

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct HookConfig {
    #[serde(default)]
    pub(crate) hooks: Vec<HookDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct HookDefinition {
    pub(crate) event: String,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) engine: Option<String>,
    #[serde(default)]
    pub(crate) script: Option<String>,
    #[serde(default)]
    pub(crate) inline_script: Option<String>,
    #[serde(default = "default_enabled")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) timeout_ms: Option<u64>,
    #[serde(default)]
    pub(crate) instruction_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct HookConfigResponse {
    pub(crate) workspace: WorkspaceInfo,
    pub(crate) path: String,
    pub(crate) exists: bool,
    pub(crate) config: HookConfig,
}

fn default_enabled() -> bool {
    true
}

pub(crate) fn workspace_hooks_path(root: &Path) -> PathBuf {
    root.join(HOOKS_DIR).join(HOOKS_FILE)
}

#[utoipa::path(
    get,
    path = "/workspace/hooks",
    tag = "night24",
    responses(
        (status = 200, description = "Current workspace hook configuration", body = HookConfigResponse),
        (status = 409, description = "No workspace is open")
    )
)]
pub(crate) async fn get_workspace_hooks(
    State(state): State<AppState>,
) -> Result<Json<HookConfigResponse>, (StatusCode, Json<serde_json::Value>)> {
    let workspace = current_workspace_info(&state).await?;
    let root = PathBuf::from(&workspace.root_path);
    let path = workspace_hooks_path(&root);
    let exists = path.is_file();
    let config = if exists {
        let content = std::fs::read_to_string(&path).map_err(|_| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to read hook config",
            )
        })?;
        serde_json::from_str(&content).map_err(|err| {
            json_error(
                StatusCode::BAD_REQUEST,
                format!("invalid hook config: {err}"),
            )
        })?
    } else {
        HookConfig { hooks: Vec::new() }
    };

    Ok(Json(HookConfigResponse {
        workspace,
        path: path.to_string_lossy().to_string(),
        exists,
        config,
    }))
}

#[utoipa::path(
    put,
    path = "/workspace/hooks",
    tag = "night24",
    request_body = HookConfig,
    responses(
        (status = 200, description = "Saved workspace hook configuration", body = HookConfigResponse),
        (status = 409, description = "No workspace is open")
    )
)]
pub(crate) async fn put_workspace_hooks(
    State(state): State<AppState>,
    Json(config): Json<HookConfig>,
) -> Result<Json<HookConfigResponse>, (StatusCode, Json<serde_json::Value>)> {
    let workspace = current_workspace_info(&state).await?;
    let root = PathBuf::from(&workspace.root_path);
    let path = workspace_hooks_path(&root);
    let parent = path.parent().ok_or_else(|| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to resolve hook config directory",
        )
    })?;
    std::fs::create_dir_all(parent).map_err(|_| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create hook config directory",
        )
    })?;
    let content = serde_json::to_string_pretty(&config).map_err(|_| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to serialize hook config",
        )
    })?;
    std::fs::write(&path, content).map_err(|_| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to write hook config",
        )
    })?;

    Ok(Json(HookConfigResponse {
        workspace,
        path: path.to_string_lossy().to_string(),
        exists: true,
        config,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_hooks_path_uses_night24_directory() {
        let path = workspace_hooks_path(Path::new("workspace"));
        assert_eq!(
            path.to_string_lossy().replace('\\', "/"),
            "workspace/.night24/hooks.json"
        );
    }
}
