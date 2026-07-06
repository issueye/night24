use std::path::{Path, PathBuf};

use axum::{extract::State, http::StatusCode, Json};
use night24_protocol::HookEvent;
use serde::{Deserialize, Serialize};

use crate::api_types::WorkspaceInfo;
use crate::workspace::current_workspace_info;
use crate::{json_error, AppState};

const HOOKS_DIR: &str = ".night24";
const HOOKS_FILE: &str = "hooks.json";
const SUPPORTED_HOOK_ENGINES: &[&str] = &["gts", "goscript"];

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
    #[serde(default)]
    pub(crate) allowed_modules: Option<Vec<String>>,
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
    validate_hook_config(&config)?;

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

fn validate_hook_config(config: &HookConfig) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    for (index, hook) in config.hooks.iter().enumerate() {
        validate_hook_definition(index, hook)?;
    }
    Ok(())
}

fn validate_hook_definition(
    index: usize,
    hook: &HookDefinition,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let event = hook.event.trim();
    if event.parse::<HookEvent>().is_err() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!("hooks[{index}].event is not supported: {}", hook.event),
        ));
    }

    if let Some(engine) = hook.engine.as_deref().map(str::trim) {
        if !is_supported_hook_engine(engine) {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                format!("hooks[{index}].engine is not supported: {engine}"),
            ));
        }
    }

    if !has_hook_entrypoint(hook) {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!("hooks[{index}] must define script or inline_script"),
        ));
    }

    if matches!(hook.timeout_ms, Some(0)) {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!("hooks[{index}].timeout_ms must be greater than 0"),
        ));
    }

    if matches!(hook.instruction_limit, Some(0)) {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!("hooks[{index}].instruction_limit must be greater than 0"),
        ));
    }

    Ok(())
}

fn has_hook_entrypoint(hook: &HookDefinition) -> bool {
    has_non_blank_value(hook.script.as_deref())
        || has_non_blank_value(hook.inline_script.as_deref())
}

fn is_supported_hook_engine(engine: &str) -> bool {
    engine.is_empty() || SUPPORTED_HOOK_ENGINES.contains(&engine)
}

fn has_non_blank_value(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
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

    #[test]
    fn hook_config_preserves_allowed_modules() {
        let config: HookConfig = serde_json::from_str(
            r#"{
                "hooks": [
                    {
                        "event": "run_started",
                        "engine": "gts",
                        "inline_script": "function execute(args) { return { outputs: [] }; }",
                        "allowed_modules": ["fs", "@std/exec"]
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(
            config.hooks[0].allowed_modules.as_deref(),
            Some(["fs".to_string(), "@std/exec".to_string()].as_slice())
        );
        let serialized = serde_json::to_value(config).unwrap();
        assert_eq!(
            serialized["hooks"][0]["allowed_modules"],
            serde_json::json!(["fs", "@std/exec"])
        );
    }

    #[test]
    fn validates_supported_hook_config() {
        let config = HookConfig {
            hooks: vec![HookDefinition {
                event: "before_tool".to_string(),
                name: Some("audit".to_string()),
                engine: Some("gts".to_string()),
                script: Some("hooks/audit.gs".to_string()),
                inline_script: None,
                enabled: true,
                timeout_ms: Some(5_000),
                instruction_limit: Some(1_000_000),
                allowed_modules: Some(vec!["@std/exec".to_string()]),
            }],
        };

        assert!(validate_hook_config(&config).is_ok());
    }

    #[test]
    fn accepts_omitted_empty_and_trimmed_supported_hook_engines() {
        for engine in [
            None,
            Some(""),
            Some("  "),
            Some(" gts "),
            Some(" goscript "),
        ] {
            let config = HookConfig {
                hooks: vec![HookDefinition {
                    event: "before_tool".to_string(),
                    name: None,
                    engine: engine.map(str::to_string),
                    script: Some("hooks/audit.gs".to_string()),
                    inline_script: None,
                    enabled: true,
                    timeout_ms: Some(5_000),
                    instruction_limit: Some(1_000_000),
                    allowed_modules: None,
                }],
            };

            assert!(validate_hook_config(&config).is_ok(), "engine={engine:?}");
        }
    }

    #[test]
    fn rejects_unsupported_hook_engine() {
        let config = HookConfig {
            hooks: vec![HookDefinition {
                event: "run_started".to_string(),
                name: None,
                engine: Some(" node ".to_string()),
                script: Some("hooks/audit.gs".to_string()),
                inline_script: None,
                enabled: true,
                timeout_ms: Some(5_000),
                instruction_limit: Some(1_000_000),
                allowed_modules: None,
            }],
        };

        let err = validate_hook_config(&config).unwrap_err();
        assert!(err.1["error"].as_str().unwrap().contains("engine"));
        assert!(err.1["error"].as_str().unwrap().contains("node"));
    }

    #[test]
    fn rejects_unsupported_hook_event() {
        let config = HookConfig {
            hooks: vec![HookDefinition {
                event: "unknown".to_string(),
                name: None,
                engine: Some("gts".to_string()),
                script: Some("hooks/audit.gs".to_string()),
                inline_script: None,
                enabled: true,
                timeout_ms: Some(5_000),
                instruction_limit: Some(1_000_000),
                allowed_modules: None,
            }],
        };

        let err = validate_hook_config(&config).unwrap_err();
        assert!(err.1["error"].as_str().unwrap().contains("event"));
    }

    #[test]
    fn rejects_hook_without_script_entrypoint() {
        let config = HookConfig {
            hooks: vec![HookDefinition {
                event: "run_started".to_string(),
                name: None,
                engine: Some("gts".to_string()),
                script: None,
                inline_script: Some("  ".to_string()),
                enabled: true,
                timeout_ms: Some(5_000),
                instruction_limit: Some(1_000_000),
                allowed_modules: None,
            }],
        };

        let err = validate_hook_config(&config).unwrap_err();
        assert!(err.1["error"]
            .as_str()
            .unwrap()
            .contains("script or inline_script"));
    }

    #[test]
    fn hook_entrypoint_accepts_script_or_inline_script_only_when_non_blank() {
        let base = HookDefinition {
            event: "run_started".to_string(),
            name: None,
            engine: Some("gts".to_string()),
            script: None,
            inline_script: None,
            enabled: true,
            timeout_ms: Some(5_000),
            instruction_limit: Some(1_000_000),
            allowed_modules: None,
        };

        assert!(!has_hook_entrypoint(&base));
        assert!(!has_hook_entrypoint(&HookDefinition {
            script: Some("  ".to_string()),
            inline_script: Some("\n\t".to_string()),
            ..base.clone()
        }));
        assert!(has_hook_entrypoint(&HookDefinition {
            script: Some("hooks/run-started.gs".to_string()),
            ..base.clone()
        }));
        assert!(has_hook_entrypoint(&HookDefinition {
            inline_script: Some("function execute(args) { return { outputs: [] }; }".to_string()),
            ..base
        }));
    }

    #[test]
    fn rejects_zero_hook_limits() {
        let config = HookConfig {
            hooks: vec![HookDefinition {
                event: "run_started".to_string(),
                name: None,
                engine: Some("gts".to_string()),
                script: None,
                inline_script: Some(
                    "function execute(args) { return { outputs: [] }; }".to_string(),
                ),
                enabled: true,
                timeout_ms: Some(0),
                instruction_limit: Some(1_000_000),
                allowed_modules: None,
            }],
        };

        let err = validate_hook_config(&config).unwrap_err();
        assert!(err.1["error"].as_str().unwrap().contains("timeout_ms"));

        let config = HookConfig {
            hooks: vec![HookDefinition {
                timeout_ms: Some(5_000),
                instruction_limit: Some(0),
                ..config.hooks[0].clone()
            }],
        };
        let err = validate_hook_config(&config).unwrap_err();
        assert!(err.1["error"]
            .as_str()
            .unwrap()
            .contains("instruction_limit"));
    }
}
