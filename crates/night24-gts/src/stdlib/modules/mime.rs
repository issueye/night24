use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{new_error, str_obj, CallContext, HashData, Object};

pub(crate) fn mime_module() -> Object {
    module(vec![
        (
            "typeByExtension",
            native("mime.typeByExtension", mime_type_by_extension),
        ),
        (
            "extensionByType",
            native("mime.extensionByType", mime_extension_by_type),
        ),
        (
            "parseMediaType",
            native("mime.parseMediaType", mime_parse_media_type),
        ),
        (
            "formatMediaType",
            native("mime.formatMediaType", mime_format_media_type),
        ),
    ])
}

pub(crate) fn mime_type_by_extension(ctx: &mut CallContext, args: &[Object]) -> Object {
    let ext = match required_string(ctx, "mime.typeByExtension", args, 0, "extension") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let normalized = ext.to_lowercase();
    let normalized = normalized.strip_prefix('.').unwrap_or(&normalized);
    match mime_lookup_ext(normalized) {
        Some(t) => str_obj(t.to_string()),
        None => Object::Undefined,
    }
}

pub(crate) fn mime_extension_by_type(ctx: &mut CallContext, args: &[Object]) -> Object {
    let typ = match required_string(ctx, "mime.extensionByType", args, 0, "type") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let lower = typ.to_lowercase();
    for (e, t) in mime_table() {
        if t == lower {
            return str_obj(format!(".{}", e));
        }
    }
    Object::Undefined
}

pub(crate) fn mime_parse_media_type(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match required_string(ctx, "mime.parseMediaType", args, 0, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut parts = value.split(';');
    let main = match parts.next() {
        Some(m) => m.trim().to_string(),
        None => return new_error(ctx.pos.clone(), "mime.parseMediaType: invalid media type"),
    };
    if !main.contains('/') {
        return new_error(ctx.pos.clone(), "mime.parseMediaType: invalid media type");
    }
    let params_hash = Rc::new(RefCell::new(HashData::default()));
    for part in parts {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            let v = v.trim().trim_matches('"');
            params_hash
                .borrow_mut()
                .set(k.trim().to_string(), str_obj(v.to_string()));
        }
    }
    let hash = Rc::new(RefCell::new(HashData::default()));
    hash.borrow_mut().set("type", str_obj(main));
    hash.borrow_mut().set("params", Object::Hash(params_hash));
    Object::Hash(hash)
}

pub(crate) fn mime_format_media_type(ctx: &mut CallContext, args: &[Object]) -> Object {
    let typ = match required_string(ctx, "mime.formatMediaType", args, 0, "type") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut out = typ;
    if let Some(Object::Hash(params)) = args.get(1) {
        for (k, v) in &params.borrow().entries {
            out.push_str(&format!("; {}=\"{}\"", k, v.inspect()));
        }
    } else if let Some(o) = args.get(1) {
        if !matches!(o, Object::Undefined | Object::Null) {
            return new_error(
                ctx.pos.clone(),
                "mime.formatMediaType: params must be an object",
            );
        }
    }
    if out.is_empty() {
        return new_error(ctx.pos.clone(), "mime.formatMediaType: invalid media type");
    }
    str_obj(out)
}

pub(crate) fn mime_lookup_ext(ext: &str) -> Option<&'static str> {
    mime_table()
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, t)| *t)
}

pub(crate) fn mime_table() -> Vec<(&'static str, &'static str)> {
    vec![
        ("txt", "text/plain"),
        ("html", "text/html"),
        ("htm", "text/html"),
        ("css", "text/css"),
        ("csv", "text/csv"),
        ("md", "text/markdown"),
        ("json", "application/json"),
        ("xml", "application/xml"),
        ("yaml", "application/yaml"),
        ("yml", "application/yaml"),
        ("js", "application/javascript"),
        ("pdf", "application/pdf"),
        ("zip", "application/zip"),
        ("gz", "application/gzip"),
        ("tar", "application/x-tar"),
        ("png", "image/png"),
        ("jpg", "image/jpeg"),
        ("jpeg", "image/jpeg"),
        ("gif", "image/gif"),
        ("svg", "image/svg+xml"),
        ("webp", "image/webp"),
        ("wav", "audio/wav"),
        ("mp3", "audio/mpeg"),
        ("mp4", "video/mp4"),
        ("webm", "video/webm"),
    ]
}

// ---------------------------------------------------------------------------
// net/ip: parse IP/CIDR + host:port helpers + DNS lookup.
// ---------------------------------------------------------------------------
