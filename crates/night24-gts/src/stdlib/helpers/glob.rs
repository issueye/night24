use super::*;

pub(crate) fn glob_paths(pattern: &str) -> Result<Vec<PathBuf>, String> {
    let normalized = pattern.replace('\\', "/");
    if !normalized.contains('*') {
        let path = PathBuf::from(pattern);
        return Ok(if path.exists() {
            vec![path]
        } else {
            Vec::new()
        });
    }
    let wildcard = normalized
        .find('*')
        .ok_or_else(|| "missing wildcard".to_string())?;
    let root_end = normalized[..wildcard]
        .rfind('/')
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let root = if root_end == 0 {
        PathBuf::from(".")
    } else {
        PathBuf::from(normalized[..root_end].replace('/', MAIN_SEPARATOR_STR))
    };
    let mut matches = Vec::new();
    glob_collect(&root, pattern, &mut matches).map_err(|e| e.to_string())?;
    matches.sort();
    Ok(matches)
}

pub(crate) fn glob_collect(
    current: &Path,
    pattern: &str,
    matches: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    if !current.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            glob_collect(&path, pattern, matches)?;
        }
        if glob_match(pattern, &path.to_string_lossy()) {
            matches.push(path);
        }
    }
    Ok(())
}

pub(crate) fn glob_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    let value = value.replace('\\', "/");
    wildcard_match(&pattern, &value)
}

pub(crate) fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut pi, mut vi) = (0usize, 0usize);
    let mut star = None;
    let mut star_match = 0usize;
    while vi < value.len() {
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == value[vi]) {
            pi += 1;
            vi += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star = Some(pi);
            star_match = vi;
            pi += 1;
        } else if let Some(star_pos) = star {
            pi = star_pos + 1;
            star_match += 1;
            vi = star_match;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}
