use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{str_obj, CallContext, HashData, Object};

pub(crate) fn highlight_module() -> Object {
    module(vec![(
        "terminal",
        native("highlight.terminal", highlight_terminal),
    )])
}

pub(crate) struct HighlightOpts {
    lang: String,
    width: usize,
    color: bool,
}

pub(crate) fn highlight_terminal(ctx: &mut CallContext, args: &[Object]) -> Object {
    let code = match required_string(ctx, "highlight.terminal", args, 0, "code") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut opts = HighlightOpts {
        lang: String::new(),
        width: 80,
        color: true,
    };
    if let Some(Object::Hash(h)) = args.get(1) {
        if let Some(Object::String(s)) = h.borrow().get("lang") {
            opts.lang = s.to_ascii_lowercase();
        }
        if let Some(Object::Number(n)) = h.borrow().get("width") {
            opts.width = *n as usize;
        }
        if let Some(Object::Boolean(b)) = h.borrow().get("color") {
            opts.color = *b;
        }
    }
    if opts.width < 1 {
        opts.width = 80;
    }

    let mut lines: Vec<String> = Vec::new();
    for raw_line in code.replace("\r\n", "\n").split('\n') {
        for wrapped in wrap_simple(raw_line, opts.width) {
            lines.push(highlight_line(&wrapped, &opts));
        }
    }

    let out = Rc::new(RefCell::new(HashData::default()));
    out.borrow_mut().set(
        "lines",
        array(lines.iter().map(|s| str_obj(s.clone())).collect()),
    );
    out.borrow_mut().set("text", str_obj(lines.join("\n")));
    out.borrow_mut().set("lang", str_obj(opts.lang.clone()));
    Object::Hash(out)
}

pub(crate) fn wrap_simple(line: &str, width: usize) -> Vec<String> {
    if width == 0 || line.chars().count() <= width {
        return vec![line.to_string()];
    }
    let mut out = Vec::new();
    let mut current = String::new();
    for (i, c) in line.chars().enumerate() {
        current.push(c);
        if (i + 1) % width == 0 {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

pub(crate) fn highlight_line(line: &str, opts: &HighlightOpts) -> String {
    if !opts.color {
        return line.to_string();
    }
    match opts.lang.as_str() {
        "diff" => {
            if line.starts_with('+') {
                return terminal_style_string(line, "success", false);
            }
            if line.starts_with('-') {
                return terminal_style_string(line, "error", false);
            }
            if line.starts_with("@@") {
                return terminal_style_string(line, "accent", true);
            }
            line.to_string()
        }
        "json" => highlight_json_line(line),
        "shell" | "sh" | "bash" | "gs" | "js" | "toml" => {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') || trimmed.starts_with("//") {
                terminal_style_string(line, "muted", false)
            } else {
                line.to_string()
            }
        }
        _ => line.to_string(),
    }
}

pub(crate) fn highlight_json_line(line: &str) -> String {
    let mut out = String::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut buf = String::new();
    for r in line.chars() {
        if in_string {
            buf.push(r);
            if escaped {
                escaped = false;
                continue;
            }
            if r == '\\' {
                escaped = true;
                continue;
            }
            if r == '"' {
                out.push_str(&terminal_style_string(&buf, "success", false));
                buf.clear();
                in_string = false;
            }
            continue;
        }
        if r == '"' {
            in_string = true;
            buf.push(r);
            continue;
        }
        out.push(r);
    }
    if !buf.is_empty() {
        out.push_str(&buf);
    }
    out
}
