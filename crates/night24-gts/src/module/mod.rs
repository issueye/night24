//! Module resolution and cache helpers.
//!
//! The runtime owns evaluation, but this module owns the stable identity of a
//! module specifier: native modules, relative source files, directory entries,
//! project import aliases, package exports, and JSON files.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::object::Object;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleKind {
    Native,
    Source,
    Json,
    Package,
    /// A module resolved from inside a `.gspkg` archive (E1.1).
    PackageArchive,
}

#[derive(Debug, Clone)]
pub struct ResolvedModule {
    pub id: String,
    pub kind: ModuleKind,
    pub specifier: String,
    pub path: Option<PathBuf>,
    pub package_root: Option<PathBuf>,
    pub package_name: String,
    /// For `.gspkg` archive modules (E1.1): the on-disk archive path and the
    /// in-archive entry name. `path` holds `archive!entry` for display.
    pub archive: Option<ArchiveRef>,
}

/// A reference to a module inside a `.gspkg` archive.
#[derive(Debug, Clone)]
pub struct ArchiveRef {
    pub archive_path: PathBuf,
    pub entry_name: String,
}

#[derive(Debug, Clone, Default)]
pub struct ResolveOptions {
    pub project_root: Option<PathBuf>,
    pub base_dir: Option<PathBuf>,
    pub referrer: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
struct Manifest {
    name: String,
    entry: String,
    package_name: String,
    package_main: String,
    package_version: String,
    exports: HashMap<String, String>,
    imports: HashMap<String, String>,
    dependencies: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ResolveCacheKey {
    specifier: String,
    project_root: Option<PathBuf>,
    base_dir: PathBuf,
    referrer: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct ResolveCacheEntry {
    resolved: Result<ResolvedModule, String>,
}

#[derive(Default)]
pub struct Resolver {
    project_root: Option<PathBuf>,
    project_roots: RefCell<HashMap<PathBuf, Option<PathBuf>>>,
    manifests: RefCell<HashMap<PathBuf, Result<Manifest, String>>>,
    resolutions: RefCell<HashMap<ResolveCacheKey, ResolveCacheEntry>>,
}

impl Resolver {
    pub fn new(project_root: impl Into<Option<PathBuf>>) -> Self {
        Self {
            project_root: project_root.into().map(clean_path),
            ..Self::default()
        }
    }

    pub fn resolve(
        &self,
        specifier: &str,
        options: ResolveOptions,
    ) -> Result<ResolvedModule, String> {
        let base_dir = options
            .base_dir
            .or_else(|| options.referrer.as_deref().and_then(base_dir_from_referrer))
            .or_else(|| std::env::current_dir().ok())
            .map(clean_path)
            .unwrap_or_else(|| PathBuf::from("."));
        let project_root = options
            .project_root
            .or_else(|| self.project_root.clone())
            .or_else(|| self.find_project_root(&base_dir));
        let key = ResolveCacheKey {
            specifier: specifier.to_string(),
            project_root: project_root.clone(),
            base_dir: base_dir.clone(),
            referrer: options.referrer.map(clean_path),
        };
        if let Some(entry) = self.resolutions.borrow().get(&key).cloned() {
            return entry.resolved;
        }
        let resolved = self.resolve_uncached(specifier, &base_dir, project_root.as_deref());
        self.resolutions.borrow_mut().insert(
            key,
            ResolveCacheEntry {
                resolved: resolved.clone(),
            },
        );
        resolved
    }

    fn resolve_uncached(
        &self,
        specifier: &str,
        base_dir: &Path,
        project_root: Option<&Path>,
    ) -> Result<ResolvedModule, String> {
        if is_native_specifier(specifier) {
            return Ok(ResolvedModule {
                id: format!("native:{specifier}"),
                kind: ModuleKind::Native,
                specifier: specifier.to_string(),
                path: None,
                package_root: None,
                package_name: String::new(),
                archive: None,
            });
        }

        // `.gspkg` archive resolution (E1.1). A base dir like
        // `/path/pkg.gspkg!subdir` (or a specifier that resolves inside a
        // `.gspkg`) means the module lives in a packaged archive.
        let base_str = base_dir.to_string_lossy();
        if let Some((pkg_path, archive_base)) = split_archive_base_dir(&base_str) {
            if pkg_path.extension().and_then(|e| e.to_str()) == Some("gspkg") {
                return self.resolve_archive_relative(specifier, &pkg_path, &archive_base);
            }
        }
        // A specifier itself pointing into a `.gspkg`: `pkg.gspkg!path`. The
        // archive path may be relative to base_dir, so join it.
        if let Some((pkg_path, archive_base)) = split_archive_base_dir(specifier) {
            if pkg_path.extension().and_then(|e| e.to_str()) == Some("gspkg") {
                let pkg_path = if pkg_path.is_absolute() {
                    pkg_path
                } else {
                    clean_path(base_dir.join(pkg_path))
                };
                return self.resolve_archive_relative(specifier, &pkg_path, &archive_base);
            }
        }

        if is_path_specifier(specifier) {
            let base = path_from_specifier(specifier);
            let candidate = if base.is_absolute() {
                base
            } else {
                base_dir.join(base)
            };
            let (path, kind) = resolve_source_path(&candidate).ok_or_else(|| {
                format!(
                    "cannot resolve module '{}' from {}",
                    specifier,
                    base_dir.display()
                )
            })?;
            return Ok(source_module(
                specifier,
                path,
                kind,
                ModuleKind::Source,
                None,
                "",
            ));
        }

        if let Some(resolved) = self.try_resolve_import_alias(specifier, base_dir, project_root)? {
            return Ok(resolved);
        }

        self.resolve_package(specifier, base_dir, project_root)
    }

    /// Resolve a module relative to a `.gspkg` archive's internal base dir.
    /// `archive_base` already holds the in-archive path (it may be the full
    /// path when the specifier itself was `pkg.gspkg!path`, or a subdir when
    /// the base dir was `pkg.gspkg!subdir` and the specifier is relative).
    fn resolve_archive_relative(
        &self,
        specifier: &str,
        pkg_path: &Path,
        archive_base: &str,
    ) -> Result<ResolvedModule, String> {
        // The in-archive candidate is `archive_base` itself when the specifier
        // was the full `pkg!path` form (archive_base == the path). When the
        // specifier is a relative sub-path joined under archive_base, it is
        // already combined by the caller's split; so use archive_base directly.
        let base = normalize_archive_dir(archive_base);
        let entry_name = crate::packagefile::resolve_archive_entry_name(pkg_path, &base)
            .ok_or_else(|| {
                format!(
                    "module '{}' not found in archive '{}' (base '{}')",
                    specifier,
                    pkg_path.display(),
                    archive_base
                )
            })?;
        let display_path = format!("{}!{}", pkg_path.display(), entry_name);
        Ok(ResolvedModule {
            id: format!("archive:{}!{}", pkg_path.display(), entry_name),
            kind: ModuleKind::PackageArchive,
            specifier: specifier.to_string(),
            path: Some(PathBuf::from(display_path)),
            package_root: Some(pkg_path.to_path_buf()),
            package_name: String::new(),
            archive: Some(ArchiveRef {
                archive_path: pkg_path.to_path_buf(),
                entry_name,
            }),
        })
    }

    fn try_resolve_import_alias(
        &self,
        specifier: &str,
        base_dir: &Path,
        project_root: Option<&Path>,
    ) -> Result<Option<ResolvedModule>, String> {
        let root = project_root
            .map(Path::to_path_buf)
            .or_else(|| self.find_project_root(base_dir));
        let Some(root) = root else {
            return Ok(None);
        };
        let manifest = match self.load_manifest(&root) {
            Ok(manifest) => manifest,
            Err(_) => return Ok(None),
        };
        let Some(target) = match_pattern_map(&manifest.imports, specifier) else {
            return Ok(None);
        };
        let (path, kind) = resolve_source_path(&root.join(path_from_specifier(&target)))
            .ok_or_else(|| format!("package import '{}' not found", specifier))?;
        Ok(Some(source_module(
            specifier,
            path,
            kind,
            ModuleKind::Source,
            Some(root),
            &package_name(&manifest, ""),
        )))
    }

    fn resolve_package(
        &self,
        specifier: &str,
        base_dir: &Path,
        project_root: Option<&Path>,
    ) -> Result<ResolvedModule, String> {
        let root = project_root
            .map(Path::to_path_buf)
            .or_else(|| self.find_project_root(base_dir))
            .ok_or_else(|| {
                format!(
                    "package '{}' cannot be resolved outside a project",
                    specifier
                )
            })?;
        let manifest = self.load_manifest(&root)?;
        let (package, export_name) = split_package_specifier(specifier);
        let source = manifest
            .dependencies
            .get(&package)
            .ok_or_else(|| format!("package '{}' is not listed in dependencies", package))?
            .clone();
        let dep_root = dependency_root(&root, &source)?;
        let dep_manifest = self.load_manifest(&dep_root)?;
        let target = export_target(&dep_manifest, &export_name)
            .ok_or_else(|| format!("package '{}' has no export '{}'", package, export_name))?;
        let (path, kind) = resolve_source_path(&dep_root.join(path_from_specifier(&target)))
            .ok_or_else(|| format!("package '{}' export '{}' not found", package, export_name))?;
        let name = package_name(&dep_manifest, &package);
        let mut resolved = source_module(
            specifier,
            path,
            kind,
            ModuleKind::Package,
            Some(dep_root),
            &name,
        );
        if !name.is_empty() {
            let version = dep_manifest.package_version;
            resolved.id = if version.is_empty() {
                format!("pkg:{}:{}", name, export_name)
            } else {
                format!("pkg:{}@{}:{}", name, version, export_name)
            };
        }
        Ok(resolved)
    }

    fn find_project_root(&self, start_dir: &Path) -> Option<PathBuf> {
        let start = clean_path(start_dir);
        if let Some(cached) = self.project_roots.borrow().get(&start).cloned() {
            return cached;
        }
        let root = find_project_root(&start);
        self.project_roots.borrow_mut().insert(start, root.clone());
        root
    }

    fn load_manifest(&self, root: &Path) -> Result<Manifest, String> {
        let root = clean_path(root);
        if let Some(cached) = self.manifests.borrow().get(&root).cloned() {
            return cached;
        }
        let loaded = read_manifest(&root);
        self.manifests.borrow_mut().insert(root, loaded.clone());
        loaded
    }
}

pub type ModuleCache = RefCell<HashMap<String, Object>>;

pub fn new_module_cache() -> ModuleCache {
    RefCell::new(HashMap::new())
}

pub fn cache_get(cache: &ModuleCache, id: &str) -> Option<Object> {
    cache.borrow().get(id).cloned()
}

pub fn cache_insert(cache: &ModuleCache, id: impl Into<String>, module: Object) {
    cache.borrow_mut().insert(id.into(), module);
}

pub fn find_project_root(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = clean_path(start_dir);
    loop {
        if read_manifest(&dir).is_ok() {
            return Some(dir);
        }
        let parent = dir.parent()?.to_path_buf();
        if parent == dir {
            return None;
        }
        dir = parent;
    }
}

pub fn resolve_entry_in_dir(dir: &Path) -> Option<PathBuf> {
    let manifest = read_manifest(dir).ok();
    if let Some(manifest) = manifest {
        if let Some(main) = package_main(&manifest) {
            if let Some((path, _)) = resolve_source_path(&dir.join(path_from_specifier(&main))) {
                return Some(path);
            }
        }
    }
    for name in ["index.gs", "main.gs"] {
        let path = dir.join(name);
        if path.is_file() {
            return Some(clean_path(path));
        }
    }
    None
}

fn resolve_source_path(candidate: &Path) -> Option<(PathBuf, ModuleKind)> {
    let candidate = clean_path(candidate);
    if candidate.is_file() {
        return Some((candidate.clone(), kind_for_path(&candidate)));
    }
    if candidate.extension().is_none() {
        for ext in ["gs", "json"] {
            let with_ext = candidate.with_extension(ext);
            if with_ext.is_file() {
                return Some((clean_path(&with_ext), kind_for_path(&with_ext)));
            }
        }
    }
    if candidate.is_dir() {
        return resolve_entry_in_dir(&candidate).map(|path| {
            let kind = kind_for_path(&path);
            (path, kind)
        });
    }
    None
}

fn source_module(
    specifier: &str,
    path: PathBuf,
    source_kind: ModuleKind,
    logical_kind: ModuleKind,
    package_root: Option<PathBuf>,
    package_name: &str,
) -> ResolvedModule {
    let path = clean_path(path);
    let id = match logical_kind {
        ModuleKind::Package if !package_name.is_empty() => {
            format!("pkg:{}:{}", package_name, path.to_string_lossy())
        }
        _ => format!("file:{}", path.to_string_lossy().replace('\\', "/")),
    };
    ResolvedModule {
        id,
        kind: match source_kind {
            ModuleKind::Json => ModuleKind::Json,
            _ => logical_kind,
        },
        specifier: specifier.to_string(),
        path: Some(path),
        package_root,
        package_name: package_name.to_string(),
        archive: None,
    }
}

fn kind_for_path(path: &Path) -> ModuleKind {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("json") => ModuleKind::Json,
        _ => ModuleKind::Source,
    }
}

fn read_manifest(root: &Path) -> Result<Manifest, String> {
    let text = fs::read_to_string(root.join("project.toml"))
        .map_err(|e| format!("cannot read {}: {}", root.join("project.toml").display(), e))?;
    Ok(parse_manifest(&text))
}

fn parse_manifest(text: &str) -> Manifest {
    let mut manifest = Manifest::default();
    let mut section = String::new();
    for raw in text.lines() {
        let line = strip_toml_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches(&['[', ']'][..]).trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_matches('"').to_string();
        let value = unquote_toml_string(value.trim());
        match section.as_str() {
            "" | "project" => match key.as_str() {
                "name" => manifest.name = value,
                "entry" => manifest.entry = value,
                _ => {}
            },
            "package" => match key.as_str() {
                "name" => manifest.package_name = value,
                "main" => manifest.package_main = value,
                "version" => manifest.package_version = value,
                _ => {}
            },
            "exports" => {
                manifest.exports.insert(key, value);
            }
            "imports" => {
                manifest.imports.insert(key, value);
            }
            "dependencies" => {
                manifest.dependencies.insert(key, value);
            }
            _ => {}
        }
    }
    manifest
}

fn strip_toml_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '#' if !in_string => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn unquote_toml_string(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else {
        value.to_string()
    }
}

fn package_main(manifest: &Manifest) -> Option<String> {
    if !manifest.package_main.is_empty() {
        Some(manifest.package_main.clone())
    } else if !manifest.entry.is_empty() {
        Some(manifest.entry.clone())
    } else {
        None
    }
}

fn package_name(manifest: &Manifest, fallback: &str) -> String {
    if !manifest.package_name.is_empty() {
        manifest.package_name.clone()
    } else if !manifest.name.is_empty() {
        manifest.name.clone()
    } else {
        fallback.to_string()
    }
}

fn export_target(manifest: &Manifest, export_name: &str) -> Option<String> {
    if manifest.exports.is_empty() {
        if export_name != "." {
            return None;
        }
        return package_main(manifest).or_else(|| Some("index.gs".into()));
    }
    match_pattern_map(&manifest.exports, export_name)
}

fn match_pattern_map(mapping: &HashMap<String, String>, name: &str) -> Option<String> {
    if let Some(target) = mapping.get(name) {
        return Some(target.clone());
    }
    for (pattern, target) in mapping {
        let Some((prefix, suffix)) = pattern.split_once('*') else {
            continue;
        };
        if name.starts_with(prefix) && name.ends_with(suffix) {
            let matched = name.trim_start_matches(prefix).trim_end_matches(suffix);
            return Some(target.replacen('*', matched, 1));
        }
    }
    None
}

fn split_package_specifier(specifier: &str) -> (String, String) {
    let parts: Vec<&str> = specifier.split('/').collect();
    if specifier.starts_with('@') && parts.len() >= 2 {
        let name = format!("{}/{}", parts[0], parts[1]);
        let export = if parts.len() > 2 {
            format!("./{}", parts[2..].join("/"))
        } else {
            ".".into()
        };
        return (name, export);
    }
    let name = parts.first().copied().unwrap_or(specifier).to_string();
    let export = if parts.len() > 1 {
        format!("./{}", parts[1..].join("/"))
    } else {
        ".".into()
    };
    (name, export)
}

fn dependency_root(project_root: &Path, source: &str) -> Result<PathBuf, String> {
    let rel = source
        .strip_prefix("file:")
        .or_else(|| source.strip_prefix("workspace:"))
        .ok_or_else(|| format!("unsupported dependency source '{}'", source))?;
    let path = path_from_specifier(rel);
    Ok(if path.is_absolute() {
        clean_path(path)
    } else {
        clean_path(project_root.join(path))
    })
}

fn is_native_specifier(specifier: &str) -> bool {
    specifier.starts_with("@std/")
}

/// Split a `pkg.gspkg!sub/path` specifier/base-dir into `(archive path,
/// archive-internal dir)`. Returns `None` when there is no `!` separator.
/// Mirrors Go's `splitArchiveBaseDir`.
fn split_archive_base_dir(spec: &str) -> Option<(PathBuf, String)> {
    let normalized = spec.replace('\\', "/");
    let idx = normalized.rfind('!')?;
    let pkg = PathBuf::from(&normalized[..idx]);
    let archive_dir = normalize_archive_dir(&normalized[idx + 1..]);
    Some((pkg, archive_dir))
}

fn normalize_archive_dir(s: &str) -> String {
    let trimmed = s.trim_start_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.replace('\\', "/")
    }
}

fn is_path_specifier(specifier: &str) -> bool {
    specifier == "."
        || specifier == ".."
        || specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier.starts_with(".\\")
        || specifier.starts_with("..\\")
        || Path::new(specifier).is_absolute()
        || (specifier.len() >= 3
            && specifier.as_bytes()[1] == b':'
            && matches!(specifier.as_bytes()[2], b'\\' | b'/'))
}

fn path_from_specifier(specifier: &str) -> PathBuf {
    PathBuf::from(specifier.replace('\\', std::path::MAIN_SEPARATOR_STR))
}

fn base_dir_from_referrer(referrer: &Path) -> Option<PathBuf> {
    if referrer.is_dir() {
        Some(referrer.to_path_buf())
    } else {
        referrer.parent().map(Path::to_path_buf)
    }
}

fn clean_path(path: impl AsRef<Path>) -> PathBuf {
    fs::canonicalize(path.as_ref()).unwrap_or_else(|_| path.as_ref().to_path_buf())
}
