use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, Object};

pub(crate) fn semver_module() -> Object {
    module(vec![
        ("parse", native("semver.parse", semver_parse)),
        ("valid", native("semver.valid", semver_valid)),
        ("compare", native("semver.compare", semver_compare_fn)),
        ("gt", native("semver.gt", semver_gt)),
        ("gte", native("semver.gte", semver_gte)),
        ("lt", native("semver.lt", semver_lt)),
        ("lte", native("semver.lte", semver_lte)),
        ("eq", native("semver.eq", semver_eq)),
        ("neq", native("semver.neq", semver_neq)),
        ("inc", native("semver.inc", semver_inc)),
        ("satisfies", native("semver.satisfies", semver_satisfies)),
    ])
}

/// Parsed semantic version. `prerelease` segments may be numeric or textual.
pub(crate) struct Semver {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Vec<String>,
    build: Vec<String>,
}

pub(crate) fn parse_semver(value: &str) -> Option<Semver> {
    let value = value.trim();
    let value = value.strip_prefix('v').unwrap_or(value);
    // Separate build metadata first.
    let (core, build) = match value.split_once('+') {
        Some((c, b)) => (c, b),
        None => (value, ""),
    };
    let (main, prerelease) = match core.split_once('-') {
        Some((m, p)) => (m, p),
        None => (core, ""),
    };
    let nums: Vec<&str> = main.split('.').collect();
    if nums.len() != 3 {
        return None;
    }
    let major = nums[0].parse::<u64>().ok()?;
    let minor = nums[1].parse::<u64>().ok()?;
    let patch = nums[2].parse::<u64>().ok()?;
    if nums.iter().any(|n| n.is_empty()) {
        return None;
    }
    let prerelease: Vec<String> = if prerelease.is_empty() {
        Vec::new()
    } else {
        prerelease.split('.').map(|s| s.to_string()).collect()
    };
    if !prerelease.iter().all(|s| valid_pre_segment(s)) {
        return None;
    }
    let build: Vec<String> = if build.is_empty() {
        Vec::new()
    } else {
        build.split('.').map(|s| s.to_string()).collect()
    };
    if !build.iter().all(|s| valid_meta_segment(s)) {
        return None;
    }
    Some(Semver {
        major,
        minor,
        patch,
        prerelease,
        build,
    })
}

pub(crate) fn valid_pre_segment(seg: &str) -> bool {
    !seg.is_empty()
        && seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && seg.parse::<u64>().map(|_| true).unwrap_or(true)
}

pub(crate) fn valid_meta_segment(seg: &str) -> bool {
    !seg.is_empty() && seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub(crate) fn semver_to_object(sv: &Semver) -> Object {
    // Go emits numeric prerelease segments as numbers and textual ones as strings.
    let pre: Vec<Object> = sv
        .prerelease
        .iter()
        .map(|s| match s.parse::<u64>() {
            Ok(n) => num_obj(n as f64),
            Err(_) => str_obj(s.clone()),
        })
        .collect();
    ObjectBuilder::new()
        .set("major", num_obj(sv.major as f64))
        .set("minor", num_obj(sv.minor as f64))
        .set("patch", num_obj(sv.patch as f64))
        .set("prerelease", array(pre))
        .set(
            "build",
            array(sv.build.iter().map(|s| str_obj(s.clone())).collect()),
        )
        .build()
}

/// Compare two semvers, returning -1/0/1. Build metadata is ignored.
fn compare_semver(a: &Semver, b: &Semver) -> i32 {
    if a.major != b.major {
        return if a.major > b.major { 1 } else { -1 };
    }
    if a.minor != b.minor {
        return if a.minor > b.minor { 1 } else { -1 };
    }
    if a.patch != b.patch {
        return if a.patch > b.patch { 1 } else { -1 };
    }
    compare_prerelease(&a.prerelease, &b.prerelease)
}

pub(crate) fn compare_prerelease(a: &[String], b: &[String]) -> i32 {
    // A version without prerelease has higher precedence.
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return 0,
        (true, false) => return 1,
        (false, true) => return -1,
        _ => {}
    }
    let len = a.len().min(b.len());
    for i in 0..len {
        let (la, lb) = (&a[i], &b[i]);
        let na = la.parse::<u64>().ok();
        let nb = lb.parse::<u64>().ok();
        let ord = match (na, nb) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => la.cmp(lb),
        };
        match ord {
            std::cmp::Ordering::Equal => continue,
            other => {
                return match other {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Greater => 1,
                    _ => 0,
                }
            }
        }
    }
    a.len().cmp(&b.len()) as i32
}

pub(crate) fn two_semvers(
    ctx: &mut CallContext,
    name: &str,
    args: &[Object],
) -> Result<(Semver, Semver), Object> {
    let reader = ArgReader::new(ctx, name, args);
    let v1 = reader.required_string(0, "version")?;
    let v2 = reader.required_string(1, "version")?;
    match (parse_semver(&v1), parse_semver(&v2)) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => Err(new_error(
            ctx.pos.clone(),
            format!("{}: invalid version", name),
        )),
    }
}

pub(crate) fn semver_parse(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "semver.parse", args);
    let version = match reader.required_string(0, "version") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match parse_semver(&version) {
        Some(sv) => semver_to_object(&sv),
        None => new_error(
            ctx.pos.clone(),
            format!("semver.parse: invalid version: {}", version),
        ),
    }
}

pub(crate) fn semver_valid(_ctx: &mut CallContext, args: &[Object]) -> Object {
    match args.first() {
        Some(Object::String(s)) => bool_obj(parse_semver(s).is_some()),
        _ => bool_obj(false),
    }
}

pub(crate) fn semver_compare_fn(ctx: &mut CallContext, args: &[Object]) -> Object {
    match two_semvers(ctx, "semver.compare", args) {
        Ok((a, b)) => num_obj(compare_semver(&a, &b) as f64),
        Err(err) => err,
    }
}

pub(crate) fn semver_gt(ctx: &mut CallContext, args: &[Object]) -> Object {
    match two_semvers(ctx, "semver.gt", args) {
        Ok((a, b)) => bool_obj(compare_semver(&a, &b) > 0),
        Err(err) => err,
    }
}

pub(crate) fn semver_gte(ctx: &mut CallContext, args: &[Object]) -> Object {
    match two_semvers(ctx, "semver.gte", args) {
        Ok((a, b)) => bool_obj(compare_semver(&a, &b) >= 0),
        Err(err) => err,
    }
}

pub(crate) fn semver_lt(ctx: &mut CallContext, args: &[Object]) -> Object {
    match two_semvers(ctx, "semver.lt", args) {
        Ok((a, b)) => bool_obj(compare_semver(&a, &b) < 0),
        Err(err) => err,
    }
}

pub(crate) fn semver_lte(ctx: &mut CallContext, args: &[Object]) -> Object {
    match two_semvers(ctx, "semver.lte", args) {
        Ok((a, b)) => bool_obj(compare_semver(&a, &b) <= 0),
        Err(err) => err,
    }
}

pub(crate) fn semver_eq(ctx: &mut CallContext, args: &[Object]) -> Object {
    match two_semvers(ctx, "semver.eq", args) {
        Ok((a, b)) => bool_obj(compare_semver(&a, &b) == 0),
        Err(err) => err,
    }
}

pub(crate) fn semver_neq(ctx: &mut CallContext, args: &[Object]) -> Object {
    match two_semvers(ctx, "semver.neq", args) {
        Ok((a, b)) => bool_obj(compare_semver(&a, &b) != 0),
        Err(err) => err,
    }
}

pub(crate) fn semver_inc(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "semver.inc", args);
    let version = match reader.required_string(0, "version") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let release = match reader.required_string(1, "release") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let sv = match parse_semver(&version) {
        Some(sv) => sv,
        None => {
            return new_error(
                ctx.pos.clone(),
                format!("semver.parse: invalid version: {}", version),
            )
        }
    };
    match inc_semver(sv, &release) {
        Ok(version) => str_obj(version),
        Err(msg) => new_error(ctx.pos.clone(), msg),
    }
}

pub(crate) fn inc_semver(mut sv: Semver, release: &str) -> Result<String, String> {
    match release {
        "major" => {
            sv.major += 1;
            sv.minor = 0;
            sv.patch = 0;
        }
        "minor" => {
            sv.minor += 1;
            sv.patch = 0;
        }
        "patch" => sv.patch += 1,
        "prerelease" => {
            sv.patch += 1;
            sv.prerelease = vec!["0".to_string()];
        }
        other => {
            return Err(format!("semver.inc: invalid release type: {}", other));
        }
    }
    sv.build.clear();
    if release == "prerelease" {
        Ok(format!("{}.{}.{}-0", sv.major, sv.minor, sv.patch))
    } else {
        Ok(format!("{}.{}.{}", sv.major, sv.minor, sv.patch))
    }
}

pub(crate) fn semver_satisfies(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "semver.satisfies", args);
    let version = match reader.required_string(0, "version") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let range = match reader.required_string(1, "range") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let sv = match parse_semver(&version) {
        Some(sv) => sv,
        None => {
            return new_error(
                ctx.pos.clone(),
                format!("semver.parse: invalid version: {}", version),
            )
        }
    };
    match satisfies_range(&sv, range.trim()) {
        Ok(true) => bool_obj(true),
        Ok(false) => bool_obj(false),
        Err(msg) => new_error(ctx.pos.clone(), msg),
    }
}

pub(crate) fn satisfies_range(sv: &Semver, range: &str) -> Result<bool, String> {
    if let Some(rest) = range.strip_prefix('^') {
        let base = parse_semver(rest.trim()).ok_or("semver.satisfies: invalid range")?;
        return Ok(sv.major == base.major && compare_semver(sv, &base) >= 0);
    }
    if let Some(rest) = range.strip_prefix('~') {
        let base = parse_semver(rest.trim()).ok_or("semver.satisfies: invalid range")?;
        return Ok(sv.major == base.major
            && sv.minor == base.minor
            && compare_semver(sv, &base) >= 0);
    }
    let parts: Vec<&str> = range.split_whitespace().collect();
    if parts.len() >= 2 {
        let mut i = 0;
        while i + 1 < parts.len() {
            let op = parts[i];
            let rhs = match parse_semver(parts[i + 1]) {
                Some(v) => v,
                None => {
                    i += 2;
                    continue;
                }
            };
            let cmp = compare_semver(sv, &rhs);
            let ok = match op {
                ">=" => cmp >= 0,
                ">" => cmp > 0,
                "<=" => cmp <= 0,
                "<" => cmp < 0,
                "=" | "==" => cmp == 0,
                _ => true,
            };
            if !ok {
                return Ok(false);
            }
            i += 2;
        }
        return Ok(true);
    }
    // Bare version: equality.
    match parse_semver(range) {
        Some(base) => Ok(compare_semver(sv, &base) == 0),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(version: &str) -> Semver {
        parse_semver(version).expect("version should parse")
    }

    #[test]
    fn compare_prerelease_orders_numeric_textual_and_release_versions() {
        assert_eq!(
            compare_semver(&parsed("1.0.0-alpha"), &parsed("1.0.0-alpha.1")),
            -1
        );
        assert_eq!(
            compare_semver(&parsed("1.0.0-alpha.1"), &parsed("1.0.0-alpha.beta")),
            -1
        );
        assert_eq!(
            compare_semver(&parsed("1.0.0-beta.2"), &parsed("1.0.0-beta.11")),
            -1
        );
        assert_eq!(compare_semver(&parsed("1.0.0-rc.1"), &parsed("1.0.0")), -1);
        assert_eq!(
            compare_semver(&parsed("1.0.0+build.1"), &parsed("1.0.0+build.2")),
            0
        );
    }

    #[test]
    fn inc_semver_updates_core_versions_and_drops_metadata() {
        assert_eq!(
            inc_semver(parsed("1.2.3+build.1"), "major").unwrap(),
            "2.0.0"
        );
        assert_eq!(inc_semver(parsed("1.2.3"), "minor").unwrap(), "1.3.0");
        assert_eq!(inc_semver(parsed("1.2.3"), "patch").unwrap(), "1.2.4");
        assert_eq!(
            inc_semver(parsed("1.2.3"), "prerelease").unwrap(),
            "1.2.4-0"
        );
        assert_eq!(
            inc_semver(parsed("1.2.3"), "banana").unwrap_err(),
            "semver.inc: invalid release type: banana"
        );
    }

    #[test]
    fn satisfies_range_supports_core_range_forms() {
        assert!(satisfies_range(&parsed("1.2.4"), "^1.2.3").unwrap());
        assert!(!satisfies_range(&parsed("2.0.0"), "^1.2.3").unwrap());
        assert!(satisfies_range(&parsed("1.2.9"), "~1.2.3").unwrap());
        assert!(!satisfies_range(&parsed("1.3.0"), "~1.2.3").unwrap());
        assert!(satisfies_range(&parsed("1.2.4"), ">= 1.2.3 < 2.0.0").unwrap());
        assert!(!satisfies_range(&parsed("2.0.0"), ">= 1.2.3 < 2.0.0").unwrap());
        assert!(satisfies_range(&parsed("1.2.3"), "1.2.3").unwrap());
    }
}

// ---------------------------------------------------------------------------
// collections: array helpers (unique/chunk/flatten/sample/shuffle/range).
// ---------------------------------------------------------------------------
