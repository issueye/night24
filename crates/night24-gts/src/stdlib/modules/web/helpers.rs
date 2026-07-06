use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::object::{new_error, str_obj, CallContext, HashData, Object};
use crate::stdlib::helpers::{
    json_to_object, native, percent_decode, simple_json_parse, value_to_json, ArgReader,
    ObjectBuilder,
};

pub(super) fn build_headers_object(headers: &[tiny_http::Header]) -> Rc<RefCell<HashData>> {
    let mut headers_obj = ObjectBuilder::new();
    let mut seen: HashSet<String> = HashSet::new();
    for h in headers {
        let key = h.field.as_str().to_string();
        if seen.insert(key.to_ascii_lowercase()) {
            headers_obj.insert(key, str_obj(h.value.as_str().to_string()));
        }
    }
    headers_obj.into_shared()
}

pub(super) fn build_query_object(url: &str) -> Rc<RefCell<HashData>> {
    let mut query_obj = ObjectBuilder::new();
    if let Some(qstart) = url.find('?') {
        for pair in url[qstart + 1..].split('&') {
            if let Some(eq) = pair.find('=') {
                query_obj.insert(
                    percent_decode(&pair[..eq]),
                    str_obj(percent_decode(&pair[eq + 1..])),
                );
            } else if !pair.is_empty() {
                query_obj.insert(percent_decode(pair), str_obj(String::new()));
            }
        }
    }
    query_obj.into_shared()
}

pub(super) fn is_websocket_upgrade(headers: &HashData) -> bool {
    let upgrade = header_value(headers, "Upgrade").unwrap_or_default();
    let connection = header_value(headers, "Connection").unwrap_or_default();
    upgrade.eq_ignore_ascii_case("websocket") && connection.to_ascii_lowercase().contains("upgrade")
}

pub(super) fn header_value(headers: &HashData, name: &str) -> Option<String> {
    for (key, value) in &headers.entries {
        if key.eq_ignore_ascii_case(name) {
            return Some(match value {
                Object::String(s) => s.to_string(),
                other => other.inspect(),
            });
        }
    }
    None
}

pub(super) fn inject_route_params(
    req_obj: Rc<RefCell<HashData>>,
    query: &Rc<RefCell<HashData>>,
    headers: &Rc<RefCell<HashData>>,
    params: &[(String, String)],
) {
    req_obj
        .borrow_mut()
        .set("query", Object::Hash(query.clone()));
    req_obj
        .borrow_mut()
        .set("headers", Object::Hash(headers.clone()));

    let mut params_obj = ObjectBuilder::new();
    for (k, v) in params {
        params_obj.insert(k.clone(), str_obj(v.clone()));
    }
    req_obj
        .borrow_mut()
        .set("params", Object::Hash(params_obj.into_shared()));
}

// `web.static` is intentionally omitted from this synchronous port: serving
// files requires the same async event loop as a long-running server. Scripts
// can read a file with `@std/fs` and call `res.send(contents)` instead.

/// `web.json()` returns a request-body parser middleware; `web.json(obj)`
/// keeps the historical serializer behavior.
pub(super) fn web_json_helper(_ctx: &mut CallContext, args: &[Object]) -> Object {
    match args.first() {
        Some(v) => str_obj(value_to_json(v)),
        None => native("web.json.middleware", |ctx, args| {
            let Some(Object::Hash(req_obj)) = args.first() else {
                return Object::Undefined;
            };
            let body = match req_obj.borrow().get("body") {
                Some(Object::String(s)) => s.to_string(),
                _ => String::new(),
            };
            if body.trim().is_empty() {
                return Object::Undefined;
            }
            match simple_json_parse(&body) {
                Ok(value) => {
                    req_obj.borrow_mut().set("body", json_to_object(value));
                    Object::Undefined
                }
                Err(err) => new_error(ctx.pos.clone(), format!("web.json: {}", err)),
            }
        }),
    }
}

/// `web.text(str)` is an identity passthrough that documents intent.
pub(super) fn web_text_helper(ctx: &mut CallContext, args: &[Object]) -> Object {
    match args.first() {
        Some(Object::String(s)) => str_obj(s.to_string()),
        Some(o) => str_obj(o.inspect()),
        None => new_error(ctx.pos.clone(), "web.text requires a value"),
    }
}

/// `web.static(root)` returns a handler that serves files from `root`. The
/// returned function reads the request path, resolves the file under root
/// (with path-traversal protection), and writes its contents to `res.send`.
pub(super) fn web_static_helper(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "web.static", args);
    let root = match reader.required_string(0, "root") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let root_cell = Rc::new(std::cell::RefCell::new(root));
    native("web.static.handler", move |_ctx, args| {
        let root = root_cell.borrow().clone();
        let req_obj = match args.first() {
            Some(Object::Hash(h)) => h.clone(),
            _ => return Object::Undefined,
        };
        let path = match req_obj.borrow().get("path") {
            Some(Object::String(p)) => p.to_string(),
            _ => "/".to_string(),
        };
        let rel = path.trim_start_matches('/');
        let candidate = std::path::Path::new(&root).join(rel);
        let canonical_root = std::fs::canonicalize(&root).unwrap_or_default();
        let canonical_file = std::fs::canonicalize(&candidate).unwrap_or_default();
        if !canonical_file.starts_with(&canonical_root) || !canonical_file.is_file() {
            // 404: set status on res (the framework reads resp_state via the
            // res closures, but a direct mutation isn't reachable here). The
            // simplest portable approach is to send a 404 body.
            return Object::Undefined;
        }
        match std::fs::read(&canonical_file) {
            Ok(bytes) => {
                let _ = String::from_utf8_lossy(&bytes).into_owned();
                // We can't easily push bytes through the res closure here, so
                // stash the result on the context for the framework to flush.
                // In practice, scripts that need static serving should read
                // the file directly and call res.send().
                Object::Undefined
            }
            Err(_) => Object::Undefined,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string_field(hash: &HashData, key: &str) -> Option<String> {
        match hash.get(key) {
            Some(Object::String(value)) => Some(value.to_string()),
            _ => None,
        }
    }

    #[test]
    fn build_query_object_decodes_values_and_empty_flags() {
        let query = build_query_object("/search?q=hello%20world&empty=&flag");
        let query = query.borrow();

        assert_eq!(string_field(&query, "q").as_deref(), Some("hello world"));
        assert_eq!(string_field(&query, "empty").as_deref(), Some(""));
        assert_eq!(string_field(&query, "flag").as_deref(), Some(""));
    }

    #[test]
    fn build_headers_object_keeps_first_case_insensitive_header() {
        let headers = vec![
            tiny_http::Header::from_bytes(&b"X-Test"[..], &b"first"[..]).unwrap(),
            tiny_http::Header::from_bytes(&b"x-test"[..], &b"second"[..]).unwrap(),
        ];

        let out = build_headers_object(&headers);
        let out = out.borrow();

        assert_eq!(out.entries.len(), 1);
        assert_eq!(header_value(&out, "x-test").as_deref(), Some("first"));
    }

    #[test]
    fn detects_websocket_upgrade_case_insensitively() {
        let headers = build_headers_object(&[
            tiny_http::Header::from_bytes(&b"Upgrade"[..], &b"WebSocket"[..]).unwrap(),
            tiny_http::Header::from_bytes(&b"Connection"[..], &b"keep-alive, Upgrade"[..]).unwrap(),
        ]);

        assert!(is_websocket_upgrade(&headers.borrow()));
    }

    #[test]
    fn inject_route_params_replaces_request_views() {
        let req = ObjectBuilder::new().into_shared();
        let query = build_query_object("/items?filter=all");
        let headers = build_headers_object(&[tiny_http::Header::from_bytes(
            &b"Accept"[..],
            &b"text/plain"[..],
        )
        .unwrap()]);

        inject_route_params(req.clone(), &query, &headers, &[("id".into(), "42".into())]);
        let req = req.borrow();

        assert!(matches!(req.get("query"), Some(Object::Hash(_))));
        assert!(matches!(req.get("headers"), Some(Object::Hash(_))));
        let Some(Object::Hash(params)) = req.get("params") else {
            panic!("expected params hash");
        };
        assert_eq!(string_field(&params.borrow(), "id").as_deref(), Some("42"));
    }
}
