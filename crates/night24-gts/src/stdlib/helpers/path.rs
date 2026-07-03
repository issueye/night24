use super::*;

pub(crate) fn normalize_path_string(value: &str) -> String {
    let path = PathBuf::from(value);
    path.components()
        .collect::<PathBuf>()
        .to_string_lossy()
        .to_string()
}

pub(crate) fn pathdiff(from: &Path, to: &Path) -> Option<PathBuf> {
    let from_components: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();
    let mut common = 0usize;
    while common < from_components.len()
        && common < to_components.len()
        && from_components[common] == to_components[common]
    {
        common += 1;
    }
    let mut result = PathBuf::new();
    for _ in common..from_components.len() {
        result.push("..");
    }
    for component in &to_components[common..] {
        result.push(component.as_os_str());
    }
    Some(if result.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        result
    })
}

// ===========================================================================
// P6 stdlib batch 1: encoding/base64, encoding/hex, hash, random, regexp,
// semver, collections, process.
//
// These are pure-algorithm modules with no network/IO heavy dependencies and
// are CI-friendly. Behavior contracts are derived from the Go originals in
// gts/internal/stdlib/*.go (see docs/full-parity-refactor-plan.md P6).
// ===========================================================================

// ---------------------------------------------------------------------------
// Byte input helpers shared by base64 / hex.
//
// Accepts a String (UTF-8 bytes), an Array of Numbers (low 8 bits each), or
// a Buffer-shaped Hash (recognized by a private marker key). Matches the Go
// `bufferBytesFromObject` contract.
// ---------------------------------------------------------------------------
