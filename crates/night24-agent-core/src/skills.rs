use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_SKILL_LIST_CHARS: usize = 8_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SkillRegistry {
    pub(super) skills: Vec<SkillRecord>,
    pub(super) warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SkillRecord {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) source: SkillSource,
    pub(super) path: String,
    pub(super) base_dir: String,
    pub(super) enabled: bool,
    pub(super) eligible: bool,
    pub(super) missing: Vec<String>,
    pub(super) user_invocable: bool,
    pub(super) model_invocable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SkillSource {
    Workspace,
    ProjectAgent,
    User,
}

impl SkillSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::ProjectAgent => "project_agent",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LoadedSkill {
    pub(super) skill: SkillRecord,
    pub(super) body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) file: Option<LoadedSkillFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LoadedSkillFile {
    pub(super) path: String,
    pub(super) content: String,
}

#[derive(Debug, Clone)]
struct ParsedSkill {
    record: SkillRecord,
}

#[derive(Debug, Default)]
struct SkillManifest {
    name: Option<String>,
    description: Option<String>,
    enabled: Option<bool>,
    user_invocable: Option<bool>,
    model_invocable: Option<bool>,
    requires_os: Vec<String>,
    requires_bins: Vec<String>,
    requires_env: Vec<String>,
}

#[derive(Debug)]
struct SkillRoot {
    source: SkillSource,
    priority: usize,
    path: PathBuf,
}

impl SkillRegistry {
    pub(super) fn load(working_dir: &Path) -> Self {
        let mut warnings = Vec::new();
        let mut by_name: HashMap<String, (usize, ParsedSkill)> = HashMap::new();

        for root in skill_roots(working_dir) {
            if !root.path.is_dir() {
                continue;
            }
            let entries = match std::fs::read_dir(&root.path) {
                Ok(entries) => entries,
                Err(err) => {
                    warnings.push(format!(
                        "failed to read skill root {}: {err}",
                        root.path.display()
                    ));
                    continue;
                }
            };

            for entry in entries.flatten() {
                let base_dir = entry.path();
                if !base_dir.is_dir() {
                    continue;
                }
                let manifest_path = base_dir.join("SKILL.md");
                if !manifest_path.is_file() {
                    continue;
                }
                match parse_skill(&manifest_path, &base_dir, root.source) {
                    Ok(parsed) => {
                        let name = parsed.record.name.clone();
                        match by_name.get(&name) {
                            Some((priority, _)) if *priority <= root.priority => {}
                            _ => {
                                by_name.insert(name, (root.priority, parsed));
                            }
                        }
                    }
                    Err(err) => warnings.push(format!(
                        "failed to parse skill {}: {err}",
                        manifest_path.display()
                    )),
                }
            }
        }

        let mut skills = by_name
            .into_values()
            .map(|(_, parsed)| parsed.record)
            .collect::<Vec<_>>();
        skills.sort_by(|a, b| {
            source_priority(a.source)
                .cmp(&source_priority(b.source))
                .then(a.name.cmp(&b.name))
        });

        Self { skills, warnings }
    }

    pub(super) fn available_for_prompt(&self) -> String {
        if !self
            .skills
            .iter()
            .any(|skill| skill.enabled && skill.eligible && skill.model_invocable)
        {
            return String::new();
        }
        let mut output =
            "Available skills. Load full instructions before following a skill:\n".to_string();
        for skill in self
            .skills
            .iter()
            .filter(|skill| skill.enabled && skill.eligible && skill.model_invocable)
        {
            let line = format!(
                "- {}: {} Path: skill://{}\n",
                skill.name, skill.description, skill.name
            );
            if output.len() + line.len() > MAX_SKILL_LIST_CHARS {
                output.push_str("- ... skill list truncated by prompt budget\n");
                break;
            }
            output.push_str(&line);
        }
        output
    }

    pub(super) fn explicit_invocation<'a>(&self, text: &'a str) -> Option<(&'a str, SkillRecord)> {
        let trimmed = text.trim_start();
        let name = if let Some(rest) = trimmed.strip_prefix("/skill ") {
            rest.split_whitespace().next()
        } else if let Some(rest) = trimmed.strip_prefix('$') {
            rest.split_whitespace().next()
        } else {
            None
        }?;
        let skill = self
            .skills
            .iter()
            .find(|skill| {
                skill.name == name && skill.enabled && skill.eligible && skill.user_invocable
            })?
            .clone();
        Some((name, skill))
    }

    pub(super) fn load_skill(&self, name: &str, file: Option<&str>) -> anyhow::Result<LoadedSkill> {
        let skill = self
            .skills
            .iter()
            .find(|skill| {
                skill.name == name && skill.enabled && skill.eligible && skill.model_invocable
            })
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("skill not found or unavailable: {name}"))?;
        let manifest_path = PathBuf::from(&skill.path);
        let (_, body) = split_skill_markdown(&std::fs::read_to_string(&manifest_path)?)?;
        let file = match file {
            Some(file) => Some(load_skill_file(&skill, file)?),
            None => None,
        };
        Ok(LoadedSkill { skill, body, file })
    }
}

fn skill_roots(working_dir: &Path) -> Vec<SkillRoot> {
    let mut roots = vec![
        SkillRoot {
            source: SkillSource::Workspace,
            priority: 0,
            path: working_dir.join(".night24").join("skills"),
        },
        SkillRoot {
            source: SkillSource::ProjectAgent,
            priority: 1,
            path: working_dir.join(".agents").join("skills"),
        },
    ];

    if let Some(home) = night24_home() {
        roots.push(SkillRoot {
            source: SkillSource::User,
            priority: 2,
            path: home.join("skills"),
        });
    }
    roots
}

fn night24_home() -> Option<PathBuf> {
    std::env::var_os("NIGHT24_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .map(|path| path.join(".night24"))
        })
}

fn parse_skill(
    manifest_path: &Path,
    base_dir: &Path,
    source: SkillSource,
) -> anyhow::Result<ParsedSkill> {
    let content = std::fs::read_to_string(manifest_path)?;
    let (manifest, _body) = split_skill_markdown(&content)?;
    let manifest = parse_manifest(&manifest);
    let fallback_name = base_dir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("skill")
        .to_string();
    let name = manifest
        .name
        .clone()
        .filter(|name| valid_skill_name(name))
        .unwrap_or(fallback_name);
    let description = manifest.description.clone().unwrap_or_default();
    let mut missing = Vec::new();
    if description.trim().is_empty() {
        missing.push("description".to_string());
    }
    missing.extend(missing_requirements(&manifest));
    let enabled = manifest.enabled.unwrap_or(true);
    let eligible = enabled && missing.is_empty();
    let record = SkillRecord {
        name,
        description,
        source,
        path: manifest_path.to_string_lossy().to_string(),
        base_dir: base_dir.to_string_lossy().to_string(),
        enabled,
        eligible,
        missing,
        user_invocable: manifest.user_invocable.unwrap_or(true),
        model_invocable: manifest.model_invocable.unwrap_or(true),
    };
    Ok(ParsedSkill { record })
}

fn split_skill_markdown(content: &str) -> anyhow::Result<(String, String)> {
    let normalized = content.replace("\r\n", "\n");
    let Some(rest) = normalized.strip_prefix("---\n") else {
        return Ok((String::new(), normalized));
    };
    let Some(end) = rest.find("\n---\n") else {
        anyhow::bail!("unterminated frontmatter");
    };
    let manifest = rest[..end].to_string();
    let body = rest[end + "\n---\n".len()..].to_string();
    Ok((manifest, body))
}

fn parse_manifest(frontmatter: &str) -> SkillManifest {
    let mut manifest = SkillManifest::default();
    let mut section: Option<String> = None;
    for raw_line in frontmatter.lines() {
        let line = raw_line.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let trimmed = line.trim();
        if let Some(key) = trimmed.strip_suffix(':') {
            section = Some(key.trim().to_string());
            continue;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            section = None;
            let key = key.trim();
            let value = value.trim();
            match key {
                "name" => manifest.name = Some(unquote(value)),
                "description" => manifest.description = Some(unquote(value)),
                "enabled" => manifest.enabled = parse_bool(value),
                "user_invocable" => manifest.user_invocable = parse_bool(value),
                "model_invocable" => manifest.model_invocable = parse_bool(value),
                "requires.os" => manifest.requires_os = parse_string_list(value),
                "requires.bins" => manifest.requires_bins = parse_string_list(value),
                "requires.env" => manifest.requires_env = parse_string_list(value),
                _ => {}
            }
            continue;
        }

        if let Some(section) = section.as_deref() {
            let item = trimmed.trim_start_matches('-').trim();
            if let Some((key, value)) = item.split_once(':') {
                if section == "requires" {
                    match key.trim() {
                        "os" => manifest.requires_os = parse_string_list(value.trim()),
                        "bins" => manifest.requires_bins = parse_string_list(value.trim()),
                        "env" => manifest.requires_env = parse_string_list(value.trim()),
                        _ => {}
                    }
                }
            }
        }
    }
    manifest
}

fn missing_requirements(manifest: &SkillManifest) -> Vec<String> {
    let mut missing = Vec::new();
    if !manifest.requires_os.is_empty() {
        let current = std::env::consts::OS;
        if !manifest
            .requires_os
            .iter()
            .any(|os| os.eq_ignore_ascii_case(current))
        {
            missing.push(format!("os:{current}"));
        }
    }
    for bin in &manifest.requires_bins {
        if !path_has_executable(bin) {
            missing.push(format!("bin:{bin}"));
        }
    }
    for env in &manifest.requires_env {
        if std::env::var_os(env).is_none() {
            missing.push(format!("env:{env}"));
        }
    }
    missing
}

fn path_has_executable(name: &str) -> bool {
    if name.trim().is_empty() {
        return true;
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(name);
        candidate.is_file()
            || (cfg!(windows) && dir.join(format!("{name}.exe")).is_file())
            || (cfg!(windows) && dir.join(format!("{name}.cmd")).is_file())
            || (cfg!(windows) && dir.join(format!("{name}.bat")).is_file())
    })
}

fn load_skill_file(skill: &SkillRecord, file: &str) -> anyhow::Result<LoadedSkillFile> {
    let relative = Path::new(file);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        anyhow::bail!("skill file path must stay inside the skill bundle");
    }
    let base_dir = PathBuf::from(&skill.base_dir);
    let path = base_dir.join(relative);
    let canonical_base = base_dir.canonicalize()?;
    let canonical_path = path.canonicalize()?;
    if !canonical_path.starts_with(canonical_base) {
        anyhow::bail!("skill file path escapes the skill bundle");
    }
    Ok(LoadedSkillFile {
        path: file.to_string(),
        content: std::fs::read_to_string(canonical_path)?,
    })
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn parse_string_list(value: &str) -> Vec<String> {
    let value = value.trim();
    if value.starts_with('[') && value.ends_with(']') {
        return value
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(unquote)
            .filter(|value| !value.is_empty())
            .collect();
    }
    if value.is_empty() {
        Vec::new()
    } else {
        vec![unquote(value)]
    }
}

fn unquote(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

fn valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn source_priority(source: SkillSource) -> usize {
    match source {
        SkillSource::Workspace => 0,
        SkillSource::ProjectAgent => 1,
        SkillSource::User => 2,
    }
}

impl std::fmt::Display for SkillSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_body() {
        let (manifest, body) = split_skill_markdown(
            "---\nname: test-skill\ndescription: Do work.\nenabled: true\n---\n# Body\n",
        )
        .unwrap();
        let parsed = parse_manifest(&manifest);
        assert_eq!(parsed.name.as_deref(), Some("test-skill"));
        assert_eq!(parsed.description.as_deref(), Some("Do work."));
        assert_eq!(parsed.enabled, Some(true));
        assert_eq!(body, "# Body\n");
    }

    #[test]
    fn rejects_path_escape() {
        let skill = SkillRecord {
            name: "x".to_string(),
            description: "x".to_string(),
            source: SkillSource::Workspace,
            path: "SKILL.md".to_string(),
            base_dir: ".".to_string(),
            enabled: true,
            eligible: true,
            missing: Vec::new(),
            user_invocable: true,
            model_invocable: true,
        };
        assert!(load_skill_file(&skill, "../Cargo.toml").is_err());
    }
}
