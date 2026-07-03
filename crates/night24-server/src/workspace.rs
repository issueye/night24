use std::path::{Path as FsPath, PathBuf};
use std::process::Command;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};

use crate::api_types::{
    FileTreeNode, OpenWorkspaceRequest, RecentWorkspacesResponse, WorkspaceChangeSnapshot,
    WorkspaceDiffQuery, WorkspaceDiffResponse, WorkspaceFileResponse, WorkspaceInfo,
    WorkspacePathQuery, WorkspaceStatusFile, WorkspaceStatusResponse, WorkspaceTreeResponse,
};
use crate::{json_error, AppState};

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_TREE_DEPTH: usize = 3;
const MAX_TREE_CHILDREN: usize = 200;

pub(crate) fn workspace_recents_limit() -> usize {
    std::env::var("NIGHT24_WORKSPACE_RECENTS_LIMIT")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|limit| *limit > 0)
        .unwrap_or(10)
}

pub(crate) fn canonical_workspace_root(
    path: &str,
) -> Result<PathBuf, (StatusCode, Json<serde_json::Value>)> {
    let raw = PathBuf::from(path);
    let canonical = raw
        .canonicalize()
        .map_err(|_| json_error(StatusCode::BAD_REQUEST, "workspace path does not exist"))?;
    if !canonical.is_dir() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "workspace path is not a directory",
        ));
    }
    Ok(canonical)
}

pub(crate) async fn current_workspace_info(
    state: &AppState,
) -> Result<WorkspaceInfo, (StatusCode, Json<serde_json::Value>)> {
    state
        .workspace_state
        .read()
        .await
        .current
        .clone()
        .ok_or_else(|| json_error(StatusCode::CONFLICT, "no workspace is open"))
}

pub(crate) async fn current_workspace_path(state: &AppState) -> Option<PathBuf> {
    state
        .workspace_state
        .read()
        .await
        .current
        .as_ref()
        .map(|workspace| PathBuf::from(&workspace.root_path))
}

pub(crate) fn resolve_workspace_existing_path(
    root: &FsPath,
    relative: Option<&str>,
) -> Result<PathBuf, (StatusCode, Json<serde_json::Value>)> {
    let canonical_root = root.canonicalize().map_err(|_| {
        json_error(
            StatusCode::CONFLICT,
            "current workspace root is unavailable",
        )
    })?;
    let relative = relative.unwrap_or("").trim();
    let relative_path = if relative.is_empty() || relative == "." {
        PathBuf::new()
    } else {
        let path = PathBuf::from(relative);
        if path.is_absolute() {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "workspace paths must be relative",
            ));
        }
        path
    };

    let candidate = canonical_root.join(relative_path);
    let canonical_candidate = candidate
        .canonicalize()
        .map_err(|_| json_error(StatusCode::NOT_FOUND, "workspace path not found"))?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "path escapes workspace root",
        ));
    }
    Ok(canonical_candidate)
}

pub(crate) fn workspace_relative_path(root: &FsPath, path: &FsPath) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let value = relative.to_string_lossy().replace('\\', "/");
    if value.is_empty() {
        ".".to_string()
    } else {
        value
    }
}

pub(crate) fn build_file_tree(
    root: &FsPath,
    path: &FsPath,
    depth: usize,
) -> Result<FileTreeNode, (StatusCode, Json<serde_json::Value>)> {
    let metadata = std::fs::metadata(path).map_err(|_| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to read workspace tree",
        )
    })?;
    let kind = if metadata.is_dir() {
        "directory"
    } else {
        "file"
    }
    .to_string();
    let name = if path == root {
        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("workspace")
            .to_string()
    } else {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    };

    let children = if metadata.is_dir() && depth < MAX_TREE_DEPTH {
        let mut entries = std::fs::read_dir(path)
            .map_err(|_| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to read workspace tree",
                )
            })?
            .filter_map(|entry| entry.ok())
            .filter(|entry| !should_skip_workspace_entry(&entry.path()))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| {
            let path = entry.path();
            let is_file = path.is_file();
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            (is_file, name)
        });

        let nodes = entries
            .into_iter()
            .take(MAX_TREE_CHILDREN)
            .filter_map(|entry| build_file_tree(root, &entry.path(), depth + 1).ok())
            .collect::<Vec<_>>();
        Some(nodes)
    } else if metadata.is_dir() {
        Some(Vec::new())
    } else {
        None
    };

    Ok(FileTreeNode {
        name,
        path: workspace_relative_path(root, path),
        kind,
        children,
    })
}

pub(crate) fn should_skip_workspace_entry(path: &FsPath) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            matches!(
                name,
                ".git" | "target" | "node_modules" | ".venv" | "venv" | "__pycache__"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn is_binary_bytes(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|byte| *byte == 0)
}

pub(crate) fn run_git(
    root: &FsPath,
    args: &[&str],
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to run git"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            "git command failed".to_string()
        } else {
            stderr
        };
        return Err(json_error(StatusCode::BAD_REQUEST, message));
    }

    String::from_utf8(output.stdout)
        .map_err(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "git output is not utf-8"))
}

pub(crate) fn ensure_git_workspace(
    root: &FsPath,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let inside = run_git(root, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.trim() == "true" {
        Ok(())
    } else {
        Err(json_error(
            StatusCode::BAD_REQUEST,
            "workspace is not a git repository",
        ))
    }
}

pub(crate) fn parse_git_status(raw: &str) -> Vec<WorkspaceStatusFile> {
    raw.lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let index_status = line.chars().next().unwrap_or(' ').to_string();
            let worktree_status = line.chars().nth(1).unwrap_or(' ').to_string();
            let path = line[3..].split(" -> ").last().unwrap_or("").to_string();
            if path.is_empty() {
                None
            } else {
                Some(WorkspaceStatusFile {
                    path,
                    index_status,
                    worktree_status,
                })
            }
        })
        .collect()
}

pub(crate) fn diff_stats(diff: &str) -> (usize, usize, usize) {
    let mut files_changed = 0;
    let mut insertions = 0;
    let mut deletions = 0;

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            files_changed += 1;
        } else if line.starts_with('+') && !line.starts_with("+++") {
            insertions += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            deletions += 1;
        }
    }

    (files_changed, insertions, deletions)
}

pub(crate) fn workspace_change_snapshot(
    root: &FsPath,
) -> Result<WorkspaceChangeSnapshot, (StatusCode, Json<serde_json::Value>)> {
    ensure_git_workspace(root)?;
    Ok(WorkspaceChangeSnapshot {
        status: run_git(root, &["status", "--porcelain=v1"])?,
        diff: run_git(root, &["diff", "--no-ext-diff"])?,
    })
}

pub(crate) fn build_diff_ready_event(
    run_id: &str,
    seq: u64,
    root: &FsPath,
    baseline: Option<&WorkspaceChangeSnapshot>,
) -> Option<serde_json::Value> {
    let current = workspace_change_snapshot(root).ok()?;
    if baseline.is_some_and(|baseline| baseline == &current) {
        return None;
    }
    if current.status.trim().is_empty() && current.diff.trim().is_empty() {
        return None;
    }

    let (diff_files, insertions, deletions) = diff_stats(&current.diff);
    let status_files = parse_git_status(&current.status).len();
    let files_changed = diff_files.max(status_files);
    Some(serde_json::json!({
        "type": "diff_ready",
        "run_id": run_id,
        "seq": seq,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "payload": {
            "files_changed": files_changed,
            "insertions": insertions,
            "deletions": deletions,
            "summary": format!("{} files changed (+{} / -{})", files_changed, insertions, deletions)
        }
    }))
}

#[utoipa::path(
    post,
    path = "/workspaces/open",
    tag = "night24",
    request_body = OpenWorkspaceRequest,
    responses(
        (status = 200, description = "Opened workspace", body = WorkspaceInfo),
        (status = 400, description = "Invalid workspace path")
    )
)]
pub(crate) async fn open_workspace(
    State(state): State<AppState>,
    Json(req): Json<OpenWorkspaceRequest>,
) -> Result<Json<WorkspaceInfo>, (StatusCode, Json<serde_json::Value>)> {
    let root = canonical_workspace_root(&req.path)?;
    let now = chrono::Utc::now().to_rfc3339();
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("workspace")
        .to_string();
    let root_path = root.to_string_lossy().to_string();
    let workspace = WorkspaceInfo {
        id: format!("workspace-{}", uuid::Uuid::new_v4()),
        name,
        root_path: root_path.clone(),
        created_at: now.clone(),
        last_opened_at: now,
    };

    let mut guard = state.workspace_state.write().await;
    guard.recent.retain(|item| item.root_path != root_path);
    guard.recent.insert(0, workspace.clone());
    let limit = workspace_recents_limit();
    guard.recent.truncate(limit);
    guard.current = Some(workspace.clone());

    Ok(Json(workspace))
}

#[utoipa::path(
    get,
    path = "/workspaces/current",
    tag = "night24",
    responses(
        (status = 200, description = "Current workspace", body = Option<WorkspaceInfo>)
    )
)]
pub(crate) async fn current_workspace(
    State(state): State<AppState>,
) -> Json<Option<WorkspaceInfo>> {
    Json(state.workspace_state.read().await.current.clone())
}

#[utoipa::path(
    get,
    path = "/workspaces/recent",
    tag = "night24",
    responses(
        (status = 200, description = "Recent workspaces", body = RecentWorkspacesResponse)
    )
)]
pub(crate) async fn recent_workspaces(
    State(state): State<AppState>,
) -> Json<RecentWorkspacesResponse> {
    Json(RecentWorkspacesResponse {
        workspaces: state.workspace_state.read().await.recent.clone(),
    })
}

#[utoipa::path(
    get,
    path = "/workspace/tree",
    tag = "night24",
    params(
        ("path" = Option<String>, Query, description = "Relative path under the current workspace")
    ),
    responses(
        (status = 200, description = "Workspace file tree", body = WorkspaceTreeResponse),
        (status = 409, description = "No workspace is open")
    )
)]
pub(crate) async fn workspace_tree(
    State(state): State<AppState>,
    Query(query): Query<WorkspacePathQuery>,
) -> Result<Json<WorkspaceTreeResponse>, (StatusCode, Json<serde_json::Value>)> {
    let workspace = current_workspace_info(&state).await?;
    let root = PathBuf::from(&workspace.root_path);
    let target = resolve_workspace_existing_path(&root, query.path.as_deref())?;
    if !target.is_dir() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "path is not a directory",
        ));
    }

    let node = build_file_tree(&root, &target, 0)?;
    Ok(Json(WorkspaceTreeResponse {
        workspace,
        root: node,
    }))
}

#[utoipa::path(
    get,
    path = "/workspace/file",
    tag = "night24",
    params(
        ("path" = String, Query, description = "Relative file path under the current workspace")
    ),
    responses(
        (status = 200, description = "Workspace file content", body = WorkspaceFileResponse),
        (status = 409, description = "No workspace is open")
    )
)]
pub(crate) async fn workspace_file(
    State(state): State<AppState>,
    Query(query): Query<WorkspacePathQuery>,
) -> Result<Json<WorkspaceFileResponse>, (StatusCode, Json<serde_json::Value>)> {
    let workspace = current_workspace_info(&state).await?;
    let root = PathBuf::from(&workspace.root_path);
    let relative = query
        .path
        .as_deref()
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| json_error(StatusCode::BAD_REQUEST, "missing file path"))?;
    let target = resolve_workspace_existing_path(&root, Some(relative))?;
    if !target.is_file() {
        return Err(json_error(StatusCode::BAD_REQUEST, "path is not a file"));
    }

    let metadata = std::fs::metadata(&target).map_err(|_| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to read file metadata",
        )
    })?;
    let relative_path = workspace_relative_path(&root, &target);
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    if metadata.len() > MAX_FILE_BYTES {
        return Ok(Json(WorkspaceFileResponse {
            path: relative_path,
            name,
            size: metadata.len(),
            is_binary: false,
            content: None,
            error: Some(format!("file is larger than {} bytes", MAX_FILE_BYTES)),
        }));
    }

    let bytes = std::fs::read(&target)
        .map_err(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to read file"))?;
    if is_binary_bytes(&bytes) {
        return Ok(Json(WorkspaceFileResponse {
            path: relative_path,
            name,
            size: metadata.len(),
            is_binary: true,
            content: None,
            error: Some("binary files cannot be previewed".to_string()),
        }));
    }

    match String::from_utf8(bytes) {
        Ok(content) => Ok(Json(WorkspaceFileResponse {
            path: relative_path,
            name,
            size: metadata.len(),
            is_binary: false,
            content: Some(content),
            error: None,
        })),
        Err(_) => Ok(Json(WorkspaceFileResponse {
            path: relative_path,
            name,
            size: metadata.len(),
            is_binary: true,
            content: None,
            error: Some("file is not valid UTF-8 text".to_string()),
        })),
    }
}

#[utoipa::path(
    get,
    path = "/workspace/status",
    tag = "night24",
    responses(
        (status = 200, description = "Workspace git status", body = WorkspaceStatusResponse),
        (status = 409, description = "No workspace is open")
    )
)]
pub(crate) async fn workspace_status(
    State(state): State<AppState>,
) -> Result<Json<WorkspaceStatusResponse>, (StatusCode, Json<serde_json::Value>)> {
    let workspace = current_workspace_info(&state).await?;
    let root = PathBuf::from(&workspace.root_path);
    ensure_git_workspace(&root)?;

    let branch = run_git(&root, &["branch", "--show-current"])
        .ok()
        .map(|branch| branch.trim().to_string())
        .filter(|branch| !branch.is_empty());
    let raw = run_git(&root, &["status", "--porcelain=v1"])?;
    let files = parse_git_status(&raw);
    Ok(Json(WorkspaceStatusResponse {
        workspace,
        is_git_repo: true,
        branch,
        has_changes: !files.is_empty(),
        files,
    }))
}

#[utoipa::path(
    get,
    path = "/workspace/diff",
    tag = "night24",
    params(
        ("staged" = Option<bool>, Query, description = "Return staged diff instead of worktree diff")
    ),
    responses(
        (status = 200, description = "Workspace git diff", body = WorkspaceDiffResponse),
        (status = 409, description = "No workspace is open")
    )
)]
pub(crate) async fn workspace_diff(
    State(state): State<AppState>,
    Query(query): Query<WorkspaceDiffQuery>,
) -> Result<Json<WorkspaceDiffResponse>, (StatusCode, Json<serde_json::Value>)> {
    let workspace = current_workspace_info(&state).await?;
    let root = PathBuf::from(&workspace.root_path);
    ensure_git_workspace(&root)?;

    let staged = query.staged.unwrap_or(false);
    let diff = if staged {
        run_git(&root, &["diff", "--cached", "--no-ext-diff"])?
    } else {
        run_git(&root, &["diff", "--no-ext-diff"])?
    };
    let (files_changed, insertions, deletions) = diff_stats(&diff);

    Ok(Json(WorkspaceDiffResponse {
        workspace,
        staged,
        has_changes: !diff.trim().is_empty(),
        diff,
        files_changed,
        insertions,
        deletions,
    }))
}
