use std::path::{Path, PathBuf};

pub(super) async fn run_shell_command(command: &str, working_dir: &Path) -> anyhow::Result<String> {
    let working_dir = working_dir.to_path_buf();

    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("cmd");
    #[cfg(target_os = "windows")]
    cmd.args(["/C", command]);

    #[cfg(not(target_os = "windows"))]
    let mut cmd = std::process::Command::new("sh");
    #[cfg(not(target_os = "windows"))]
    cmd.args(["-c", command]);

    let output =
        tokio::task::spawn_blocking(move || cmd.current_dir(&working_dir).output()).await??;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        if stdout.is_empty() && stderr.is_empty() {
            Ok("(command executed with no output)".to_string())
        } else if stdout.is_empty() {
            Ok(stderr)
        } else {
            Ok(stdout)
        }
    } else {
        anyhow::bail!("shell command failed: {}", stderr);
    }
}

pub(super) fn should_skip_dir(path: &std::path::Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        matches!(
            name,
            "target" | ".git" | "node_modules" | ".venv" | "venv" | "__pycache__"
        )
    } else {
        false
    }
}

pub(super) fn glob_match(pattern: &str, name: &str) -> bool {
    let mut pattern_chars = pattern.chars().peekable();
    let mut text_chars = name.chars().peekable();

    while let Some(&p) = pattern_chars.peek() {
        match p {
            '*' => {
                pattern_chars.next();
                while text_chars.peek().is_some() {
                    if glob_match(
                        pattern_chars.clone().collect::<String>().as_str(),
                        text_chars.clone().collect::<String>().as_str(),
                    ) {
                        return true;
                    }
                    text_chars.next();
                }
                return pattern_chars.clone().collect::<String>().is_empty();
            }
            '?' => {
                pattern_chars.next();
                if text_chars.next().is_none() {
                    return false;
                }
            }
            _ => {
                if text_chars.next() != Some(p) {
                    return false;
                }
                pattern_chars.next();
            }
        }
    }

    text_chars.peek().is_none()
}

pub(super) fn resolve_within_workdir(
    working_dir: &Path,
    user_path: &str,
) -> anyhow::Result<PathBuf> {
    let candidate = if Path::new(user_path).is_absolute() {
        PathBuf::from(user_path)
    } else {
        working_dir.join(user_path)
    };

    let canonical_workdir = working_dir
        .canonicalize()
        .unwrap_or_else(|_| working_dir.to_path_buf());

    // For non-existent files, canonicalize the parent directory instead.
    let canonical_candidate = candidate.canonicalize().unwrap_or_else(|_| {
        let parent = candidate
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let canonical_parent = parent.canonicalize().unwrap_or_else(|_| parent.clone());
        canonical_parent.join(candidate.file_name().unwrap_or_default())
    });

    if !canonical_candidate.starts_with(&canonical_workdir) {
        anyhow::bail!("path escapes working directory: {}", user_path);
    }

    Ok(candidate)
}
