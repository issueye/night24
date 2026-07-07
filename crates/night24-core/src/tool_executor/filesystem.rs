use std::path::{Path, PathBuf};

pub(super) async fn run_shell_command(command: &str, working_dir: &Path) -> anyhow::Result<String> {
    let working_dir = working_dir.to_path_buf();

    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("powershell.exe");
    #[cfg(target_os = "windows")]
    {
        let script = windows_powershell_command(command);
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ]);
    }

    #[cfg(not(target_os = "windows"))]
    let mut cmd = std::process::Command::new("sh");
    #[cfg(not(target_os = "windows"))]
    cmd.args(["-c", command]);

    let output =
        tokio::task::spawn_blocking(move || cmd.current_dir(&working_dir).output()).await??;

    let stdout = decode_shell_output(&output.stdout);
    let stderr = decode_shell_output(&output.stderr);
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

#[cfg(target_os = "windows")]
fn windows_powershell_command(command: &str) -> String {
    format!(
        "[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false); \
         [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
         $OutputEncoding = [System.Text.UTF8Encoding]::new($false); {}",
        command
    )
}

fn decode_shell_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
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

    // For non-existent files and directories, canonicalize the nearest existing
    // ancestor so writes can create new nested paths without escaping workdir.
    let canonical_candidate = candidate
        .canonicalize()
        .unwrap_or_else(|_| canonicalize_nonexistent_path(&candidate));

    if !canonical_candidate.starts_with(&canonical_workdir) {
        anyhow::bail!("path escapes working directory: {}", user_path);
    }

    Ok(candidate)
}

fn canonicalize_nonexistent_path(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut ancestor = path;

    while !ancestor.exists() {
        if let Some(name) = ancestor.file_name() {
            missing.push(name.to_os_string());
        }
        let Some(parent) = ancestor.parent() else {
            break;
        };
        ancestor = parent;
    }

    let mut resolved = ancestor
        .canonicalize()
        .unwrap_or_else(|_| ancestor.to_path_buf());
    for segment in missing.iter().rev() {
        resolved.push(segment);
    }
    resolved
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn windows_shell_command_decodes_utf8_chinese_output() {
        let output = run_shell_command("Write-Output '中文输出'", Path::new("."))
            .await
            .unwrap();

        assert!(output.contains("中文输出"));
    }

    #[tokio::test]
    async fn windows_shell_command_supports_powershell_syntax() {
        let output = run_shell_command("1..2 | ForEach-Object { \"项目$($_)\" }", Path::new("."))
            .await
            .unwrap();

        assert!(output.contains("项目2"));
    }
}
