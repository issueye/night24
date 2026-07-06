use std::fs;
use std::path::PathBuf;

use super::super::helpers::*;
use crate::object::{new_error, str_obj, CallContext, HashData, Object};

pub(crate) fn url_module() -> Object {
    module(vec![
        ("parse", native("url.parse", url_parse)),
        ("format", native("url.format", url_format)),
        ("resolve", native("url.resolve", url_resolve)),
        (
            "pathToFileURL",
            native("url.pathToFileURL", url_path_to_file),
        ),
        (
            "fileURLToPath",
            native("url.fileURLToPath", url_file_to_path),
        ),
    ])
}

/// Parsed URL components.
#[derive(Clone)]
pub(crate) struct UrlParts {
    scheme: String,
    host: String,
    path: String,
    query: String,
    fragment: String,
}

impl UrlParts {
    fn hostname(&self) -> String {
        // Strip a trailing ":port".
        match self.host.rfind(':') {
            Some(idx)
                if !self.host[idx + 1..].is_empty()
                    && self.host[idx + 1..].chars().all(|c| c.is_ascii_digit()) =>
            {
                self.host[..idx].to_string()
            }
            _ => self.host.clone(),
        }
    }

    fn port(&self) -> String {
        match self.host.rfind(':') {
            Some(idx)
                if !self.host[idx + 1..].is_empty()
                    && self.host[idx + 1..].chars().all(|c| c.is_ascii_digit()) =>
            {
                self.host[idx + 1..].to_string()
            }
            _ => String::new(),
        }
    }

    #[allow(clippy::inherent_to_string)]
    fn to_string(&self) -> String {
        let mut out = String::new();
        if !self.scheme.is_empty() {
            out.push_str(&self.scheme);
            out.push(':');
        }
        if !self.host.is_empty() {
            out.push_str("//");
            out.push_str(&self.host);
        }
        out.push_str(&self.path);
        if !self.query.is_empty() {
            out.push('?');
            out.push_str(&self.query);
        }
        if !self.fragment.is_empty() {
            out.push('#');
            out.push_str(&self.fragment);
        }
        out
    }

    fn to_object(&self) -> Object {
        let protocol = if self.scheme.is_empty() {
            String::new()
        } else {
            format!("{}:", self.scheme)
        };
        let search = if self.query.is_empty() {
            String::new()
        } else {
            format!("?{}", self.query)
        };
        let hash_field = if self.fragment.is_empty() {
            String::new()
        } else {
            format!("#{}", self.fragment)
        };
        let origin = if !self.scheme.is_empty() && !self.host.is_empty() {
            format!("{}://{}", self.scheme, self.host)
        } else {
            "null".to_string()
        };
        ObjectBuilder::new()
            .set("href", str_obj(self.to_string()))
            .set("protocol", str_obj(protocol))
            .set("host", str_obj(self.host.clone()))
            .set("hostname", str_obj(self.hostname()))
            .set("port", str_obj(self.port()))
            .set("pathname", str_obj(self.path.clone()))
            .set("search", str_obj(search))
            .set("hash", str_obj(hash_field))
            .set("origin", str_obj(origin))
            .build()
    }
}

/// Parse a URL into components. Implements scheme://host/path?query#fragment.
fn parse_url(input: &str) -> Option<UrlParts> {
    let (rest, fragment) = match input.split_once('#') {
        Some((r, f)) => (r, f.to_string()),
        None => (input, String::new()),
    };
    let (rest, query) = match rest.split_once('?') {
        Some((r, q)) => (r, q.to_string()),
        None => (rest, String::new()),
    };
    // Detect scheme (must be alpha leading, followed by [a-z0-9+.-]* then ':').
    let scheme_end = rest.find(':').filter(|&idx| {
        idx > 0
            && rest[..idx]
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic())
                .unwrap_or(false)
            && rest[..idx]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-')
    });
    let (scheme, after_scheme) = match scheme_end {
        Some(idx) => (rest[..idx].to_string(), &rest[idx + 1..]),
        None => (String::new(), rest),
    };
    let (host, path) = if let Some(stripped) = after_scheme.strip_prefix("//") {
        match stripped.find('/') {
            Some(slash) => (stripped[..slash].to_string(), stripped[slash..].to_string()),
            None => (stripped.to_string(), String::new()),
        }
    } else {
        (String::new(), after_scheme.to_string())
    };
    Some(UrlParts {
        scheme,
        host,
        path,
        query,
        fragment,
    })
}

pub(crate) fn url_parse(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "url.parse", args);
    let input = match reader.required_string(0, "url") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match parse_url(&input) {
        Some(parts) => parts.to_object(),
        None => new_error(
            ctx.pos.clone(),
            format!("url.parse: invalid URL: {}", input),
        ),
    }
}

pub(crate) fn url_format(ctx: &mut CallContext, args: &[Object]) -> Object {
    match args.first() {
        Some(Object::String(s)) => match parse_url(s) {
            Some(parts) => str_obj(parts.to_string()),
            None => new_error(ctx.pos.clone(), format!("url.format: invalid URL: {}", s)),
        },
        Some(Object::Hash(hash)) => {
            let h = hash.borrow();
            url_format_object(&h)
        }
        Some(_) => new_error(
            ctx.pos.clone(),
            "url.format: URL object must be an object or string",
        ),
        None => new_error(ctx.pos.clone(), "url.format requires url"),
    }
}

pub(crate) fn url_format_object(hash: &HashData) -> Object {
    str_obj(url_parts_from_object(hash).to_string())
}

pub(crate) fn url_parts_from_object(hash: &HashData) -> UrlParts {
    let mut scheme = hash_string(hash, "protocol").or_else(|| hash_string(hash, "scheme"));
    if let Some(s) = &scheme {
        if let Some(stripped) = s.strip_suffix(':') {
            scheme = Some(stripped.to_string());
        }
    }
    let host = hash_string(hash, "host").unwrap_or_else(|| {
        let hostname = hash_string(hash, "hostname").unwrap_or_default();
        let port = hash_string(hash, "port").unwrap_or_default();
        if port.is_empty() {
            hostname
        } else {
            format!("{}:{}", hostname, port)
        }
    });
    let path = hash_string(hash, "pathname")
        .or_else(|| hash_string(hash, "path"))
        .unwrap_or_default();
    let query = hash_string(hash, "search")
        .map(|s| s.strip_prefix('?').unwrap_or(&s).to_string())
        .or_else(|| hash_string(hash, "query"))
        .unwrap_or_default();
    let fragment = hash_string(hash, "hash")
        .map(|s| s.strip_prefix('#').unwrap_or(&s).to_string())
        .or_else(|| hash_string(hash, "fragment"))
        .unwrap_or_default();
    UrlParts {
        scheme: scheme.unwrap_or_default(),
        host,
        path,
        query,
        fragment,
    }
}

pub(crate) fn url_resolve(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "url.resolve", args);
    let base = match reader.required_string(0, "base") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let reference = match reader.required_string(1, "ref") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let base_parts = match parse_url(&base) {
        Some(p) => p,
        None => {
            return new_error(
                ctx.pos.clone(),
                format!("url.resolve: invalid base URL: {}", base),
            )
        }
    };
    let resolved = resolve_reference(&base_parts, &reference);
    str_obj(resolved)
}

pub(crate) fn resolve_reference(base: &UrlParts, reference: &str) -> String {
    // Absolute reference (has its own scheme) is used as-is.
    if let Some(abs) = parse_url(reference) {
        if !abs.scheme.is_empty() && abs.scheme == base.scheme && abs.host.is_empty() {
            // Scheme-relative.
            let mut merged = abs.clone();
            merged.host = base.host.clone();
            return merged.to_string();
        }
        if !abs.scheme.is_empty() {
            return abs.to_string();
        }
    }
    // Protocol-relative (//host/...).
    if let Some(rest) = reference.strip_prefix("//") {
        if let Some(abs) = parse_url(&format!("{}:{}", base.scheme, reference)) {
            return abs.to_string();
        }
        let _ = rest;
    }
    // Root-relative.
    if let Some(rest) = reference.strip_prefix('/') {
        let parts = UrlParts {
            scheme: base.scheme.clone(),
            host: base.host.clone(),
            path: format!("/{}", rest),
            query: String::new(),
            fragment: String::new(),
        };
        return parts.to_string();
    }
    // Relative path: merge with the base directory.
    let base_dir = match base.path.rfind('/') {
        Some(idx) => base.path[..=idx].to_string(),
        None => String::new(),
    };
    let mut query = String::new();
    let mut fragment = String::new();
    let (ref_path, rest) = match reference.split_once('?') {
        Some((p, q)) => (p, q),
        None => (reference, ""),
    };
    let (ref_path, frag) = match ref_path.split_once('#') {
        Some((p, f)) => (p, f.to_string()),
        None => (ref_path, String::new()),
    };
    if !rest.is_empty() {
        query = rest.to_string();
    }
    if !frag.is_empty() {
        fragment = frag;
    }
    let parts = UrlParts {
        scheme: base.scheme.clone(),
        host: base.host.clone(),
        path: format!("{}{}", base_dir, ref_path),
        query,
        fragment,
    };
    parts.to_string()
}

pub(crate) fn url_path_to_file(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "url.pathToFileURL", args);
    let path = match reader.required_string(0, "path") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let absolute = match fs::canonicalize(&path) {
        Ok(p) => p,
        Err(_) => PathBuf::from(&path),
    };
    let mut slash = absolute.to_string_lossy().replace('\\', "/");
    if cfg!(windows) && !slash.starts_with('/') {
        slash = format!("/{}", slash);
    }
    str_obj(format!("file://{}", slash))
}

pub(crate) fn url_file_to_path(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "url.fileURLToPath", args);
    let input = match reader.required_string(0, "url") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let parts = match parse_url(&input) {
        Some(p) => p,
        None => {
            return new_error(
                ctx.pos.clone(),
                format!("url.fileURLToPath: invalid URL: {}", input),
            )
        }
    };
    if parts.scheme != "file" {
        return new_error(
            ctx.pos.clone(),
            "url.fileURLToPath: URL must use file: protocol",
        );
    }
    if !parts.host.is_empty() && parts.host != "localhost" {
        return new_error(
            ctx.pos.clone(),
            "url.fileURLToPath: file URL host is not supported",
        );
    }
    let mut path = parts.path.clone();
    if cfg!(windows)
        && path.starts_with('/')
        && path.len() >= 3
        && path.as_bytes().get(2) == Some(&b':')
    {
        path = path[1..].to_string();
    }
    if cfg!(windows) {
        path = path.replace('/', "\\");
    }
    str_obj(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object_as_string(object: Object) -> String {
        match object {
            Object::String(value) => value.to_string(),
            _ => panic!("expected string"),
        }
    }

    #[test]
    fn url_format_object_builds_host_from_hostname_and_port() {
        let object = ObjectBuilder::new()
            .set("protocol", str_obj("https:"))
            .set("hostname", str_obj("example.com"))
            .set("port", str_obj("8443"))
            .set("pathname", str_obj("/docs"))
            .set("search", str_obj("?q=night24"))
            .set("hash", str_obj("#top"))
            .build();
        let Object::Hash(hash) = object else {
            panic!("expected hash");
        };

        assert_eq!(
            object_as_string(url_format_object(&hash.borrow())),
            "https://example.com:8443/docs?q=night24#top"
        );
    }

    #[test]
    fn url_format_object_prefers_host_and_ignores_non_string_fields() {
        let object = ObjectBuilder::new()
            .set("scheme", str_obj("https"))
            .set("host", str_obj("example.com"))
            .set("hostname", str_obj("ignored.test"))
            .set("pathname", Object::Boolean(true))
            .set("path", str_obj("/fallback"))
            .set("query", Object::Boolean(true))
            .set("fragment", str_obj("frag"))
            .build();
        let Object::Hash(hash) = object else {
            panic!("expected hash");
        };

        assert_eq!(
            object_as_string(url_format_object(&hash.borrow())),
            "https://example.com/fallback#frag"
        );
    }
}

// (hash_string is defined above near the env module helpers.)

// ---------------------------------------------------------------------------
// cache: a TTL dictionary with lazy expiry, matching the Go `@std/cache`
// semantics (no LRU, no capacity cap, has/size/keys include not-yet-purged
// expired entries, get lazily deletes expired entries).
// ---------------------------------------------------------------------------
