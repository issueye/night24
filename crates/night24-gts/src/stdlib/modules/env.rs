use std::env;
use std::fs;

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, Object};

pub(crate) fn env_module() -> Object {
    module(vec![
        ("load", native("env.load", env_load)),
        (
            "loadMultiple",
            native("env.loadMultiple", env_load_multiple),
        ),
        ("get", native("env.get", env_get)),
        ("getString", native("env.getString", env_get)),
        ("getInt", native("env.getInt", env_get_int)),
        ("getFloat", native("env.getFloat", env_get_float)),
        ("getNumber", native("env.getNumber", env_get_float)),
        ("getBool", native("env.getBool", env_get_bool)),
        ("getArray", native("env.getArray", env_get_array)),
        ("getJson", native("env.getJson", env_get_json)),
        ("has", native("env.has", env_has)),
        ("require", native("env.require", env_require)),
        ("set", native("env.set", env_set)),
        ("unset", native("env.unset", env_unset)),
        ("toObject", native("env.toObject", env_to_object)),
        ("parse", native("env.parse", env_parse)),
    ])
}

pub(crate) fn env_get(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "env.get", args);
    let key = match reader.required_string(0, "key") {
        Ok(value) => value,
        Err(err) => return err,
    };
    match env::var(&key) {
        Ok(value) if !value.is_empty() => str_obj(value),
        _ => args.get(1).cloned().unwrap_or(Object::Undefined),
    }
}

pub(crate) fn env_get_int(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "env.getInt", args);
    let key = match reader.required_string(0, "key") {
        Ok(value) => value,
        Err(err) => return err,
    };
    match env::var(&key).ok().filter(|v| !v.is_empty()) {
        Some(value) => value
            .parse::<i64>()
            .map(|n| num_obj(n as f64))
            .unwrap_or_else(|_| {
                args.get(1).cloned().unwrap_or_else(|| {
                    new_error(
                        ctx.pos.clone(),
                        format!("getInt: invalid integer {}", value),
                    )
                })
            }),
        None => args.get(1).cloned().unwrap_or(Object::Undefined),
    }
}

pub(crate) fn env_get_float(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "env.getFloat", args);
    let key = match reader.required_string(0, "key") {
        Ok(value) => value,
        Err(err) => return err,
    };
    match env::var(&key).ok().filter(|v| !v.is_empty()) {
        Some(value) => value.parse::<f64>().map(num_obj).unwrap_or_else(|_| {
            args.get(1).cloned().unwrap_or_else(|| {
                new_error(
                    ctx.pos.clone(),
                    format!("getFloat: invalid number {}", value),
                )
            })
        }),
        None => args.get(1).cloned().unwrap_or(Object::Undefined),
    }
}

pub(crate) fn env_get_bool(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "env.getBool", args);
    let key = match reader.required_string(0, "key") {
        Ok(value) => value,
        Err(err) => return err,
    };
    match env::var(&key)
        .ok()
        .map(|v| v.to_ascii_lowercase())
        .filter(|v| !v.is_empty())
    {
        Some(value) => match value.as_str() {
            "true" | "1" | "yes" | "on" => bool_obj(true),
            "false" | "0" | "no" | "off" => bool_obj(false),
            _ => args.get(1).cloned().unwrap_or_else(|| {
                new_error(
                    ctx.pos.clone(),
                    format!("getBool: invalid boolean {}", value),
                )
            }),
        },
        None => args.get(1).cloned().unwrap_or(Object::Undefined),
    }
}

pub(crate) fn env_get_array(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "env.getArray", args);
    let key = match reader.required_string(0, "key") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let sep = match args.get(1) {
        Some(Object::String(s)) => s.as_str(),
        _ => ",",
    };
    let Some(value) = env::var(&key).ok().filter(|v| !v.is_empty()) else {
        return array(Vec::new());
    };
    array(value.split(sep).map(|part| str_obj(part.trim())).collect())
}

pub(crate) fn env_has(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "env.has", args);
    match reader.required_string(0, "key") {
        Ok(key) => bool_obj(env::var_os(key).is_some()),
        Err(err) => err,
    }
}

pub(crate) fn env_require(ctx: &mut CallContext, args: &[Object]) -> Object {
    // Accept either a single key string or an array of required keys, matching
    // the Go `@std/env.require` contract.
    let keys: Vec<String> = match args.first() {
        Some(Object::String(s)) => vec![s.as_str().to_string()],
        Some(Object::Array(arr)) => {
            let mut out = Vec::new();
            for elem in &arr.borrow().elements {
                match elem {
                    Object::String(s) => out.push(s.as_str().to_string()),
                    _ => return new_error(ctx.pos.clone(), "env.require expects array of strings"),
                }
            }
            out
        }
        Some(_) => return new_error(ctx.pos.clone(), "env.require expects array"),
        None => return new_error(ctx.pos.clone(), "env.require requires array of keys"),
    };
    let missing: Vec<String> = keys
        .iter()
        .filter(|k| env::var(k).ok().filter(|v| !v.is_empty()).is_none())
        .cloned()
        .collect();
    if missing.is_empty() {
        Object::Undefined
    } else {
        new_error(
            ctx.pos.clone(),
            format!(
                "Missing required environment variables: {}",
                missing.join(", ")
            ),
        )
    }
}

pub(crate) fn env_load(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match args.first() {
        Some(Object::String(s)) => s.as_str().to_string(),
        _ => ".env".to_string(),
    };
    let override_existing = hash_bool_arg(args.get(1), "override").unwrap_or(false);
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return new_error(ctx.pos.clone(), format!("env.load: {}", e)),
    };
    let entries = parse_env_content(&content);
    apply_env_entries(&entries, override_existing);
    Object::Undefined
}

pub(crate) fn env_load_multiple(ctx: &mut CallContext, args: &[Object]) -> Object {
    let arr = match args.first() {
        Some(Object::Array(a)) => a.clone(),
        Some(_) => return new_error(ctx.pos.clone(), "env.loadMultiple expects array"),
        None => return new_error(ctx.pos.clone(), "env.loadMultiple requires array of paths"),
    };
    // Per the Go original, a single failing file is skipped silently.
    for elem in &arr.borrow().elements {
        if let Object::String(path) = elem {
            if let Ok(content) = fs::read_to_string(path.as_str()) {
                let entries = parse_env_content(&content);
                apply_env_entries(&entries, false);
            }
        }
    }
    Object::Undefined
}

pub(crate) fn env_get_json(ctx: &mut CallContext, args: &[Object]) -> Object {
    // The Go original's getJson is a stub returning the raw string; preserve
    // that contract for compatibility.
    let reader = ArgReader::new(ctx, "env.getJson", args);
    let key = match reader.required_string(0, "key") {
        Ok(value) => value,
        Err(err) => return err,
    };
    match env::var(&key).ok().filter(|v| !v.is_empty()) {
        Some(value) => str_obj(value),
        None => Object::Undefined,
    }
}

pub(crate) fn env_parse(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "env.parse", args);
    let content = match reader.required_string(0, "content") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let entries = parse_env_content(&content);
    let mut builder = ObjectBuilder::new();
    for (k, v) in entries {
        builder.insert(k, str_obj(v));
    }
    builder.build()
}

/// Apply parsed entries to the process environment. With override=false, only
/// keys whose current value is empty are written (matching the Go `load` rule).
fn apply_env_entries(entries: &[(String, String)], override_existing: bool) {
    for (key, value) in entries {
        let current = env::var(key).unwrap_or_default();
        if override_existing || current.is_empty() {
            env::set_var(key, value);
        }
    }
}

/// Parse `.env`-format content into ordered (key, value) pairs. Supports
/// comments, single/double quotes (including multi-line double-quoted values),
/// and `${VAR}` expansion against already-parsed entries.
fn parse_env_content(content: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, rest)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        let mut value = rest.trim().to_string();
        // Multi-line double-quoted value continues until a closing quote.
        if value.starts_with('"') && !value[1..].ends_with('"') {
            let mut buf = value.clone();
            while i < lines.len() {
                buf.push('\n');
                buf.push_str(lines[i]);
                i += 1;
                if lines[i - 1].trim_end().ends_with('"') {
                    break;
                }
            }
            value = buf;
        }
        // Strip surrounding quotes (single or double, but only when balanced).
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"')
            || value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'')
        {
            value = value[1..value.len() - 1].to_string();
        }
        // Expand ${VAR} using already-parsed entries.
        value = expand_env_vars(&value, &out);
        out.push((key, value));
    }
    out
}

pub(crate) fn expand_env_vars(value: &str, parsed: &[(String, String)]) -> String {
    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(end) = value[i + 2..].find('}') {
                let name = &value[i + 2..i + 2 + end];
                let resolved = parsed
                    .iter()
                    .rev()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                out.push_str(&resolved);
                i += 2 + end + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

pub(crate) fn env_set(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "env.set", args);
    let key = match reader.required_string(0, "key") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let value = match args.get(1) {
        Some(value) => value.inspect(),
        None => return new_error(ctx.pos.clone(), "env.set requires value"),
    };
    env::set_var(key, value);
    Object::Undefined
}

pub(crate) fn env_unset(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "env.unset", args);
    match reader.required_string(0, "key") {
        Ok(key) => {
            env::remove_var(key);
            Object::Undefined
        }
        Err(err) => err,
    }
}

pub(crate) fn env_to_object(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let mut builder = ObjectBuilder::new();
    for (key, value) in env::vars() {
        builder.insert(key, str_obj(value));
    }
    builder.build()
}
