use super::super::helpers::*;
use crate::object::{num_obj, str_obj, CallContext, Object};

pub(crate) fn text_module() -> Object {
    module(vec![
        ("chars", native("text.chars", text_chars)),
        ("runes", native("text.chars", text_chars)),
        ("width", native("text.width", text_width)),
        (
            "truncateWidth",
            native("text.truncateWidth", text_truncate_width),
        ),
        (
            "padRightWidth",
            native("text.padRightWidth", text_pad_right_width),
        ),
        ("wrapWidth", native("text.wrapWidth", text_wrap_width)),
        ("stripAnsi", native("text.stripAnsi", text_strip_ansi)),
    ])
}

pub(crate) fn text_chars(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match required_string(ctx, "text.chars", args, 0, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let stripped = strip_ansi(&value);
    let mut chars: Vec<Object> = Vec::new();
    let mut pending = String::new();
    for r in stripped.chars() {
        if is_combining_rune(r) {
            pending.push(r);
            continue;
        }
        if !pending.is_empty() {
            chars.push(str_obj(pending.clone()));
            pending.clear();
        }
        pending.push(r);
    }
    if !pending.is_empty() {
        chars.push(str_obj(pending));
    }
    array(chars)
}

pub(crate) fn text_width(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match required_string(ctx, "text.width", args, 0, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let stripped = strip_ansi(&value);
    let mut total = 0usize;
    for r in stripped.chars() {
        total += rune_width(r);
    }
    num_obj(total as f64)
}

pub(crate) fn text_truncate_width(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match required_string(ctx, "text.truncateWidth", args, 0, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let width = match required_number(ctx, "text.truncateWidth", args, 1, "width") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let limit = if width < 0.0 { 0 } else { width as usize };
    let stripped = strip_ansi(&value);
    let mut out = String::new();
    let mut used = 0usize;
    for r in stripped.chars() {
        let w = rune_width(r);
        if used + w > limit {
            break;
        }
        out.push(r);
        used += w;
    }
    str_obj(out)
}

pub(crate) fn text_pad_right_width(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match required_string(ctx, "text.padRightWidth", args, 0, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let width = match required_number(ctx, "text.padRightWidth", args, 1, "width") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let target = if width < 0.0 { 0 } else { width as usize };
    let stripped = strip_ansi(&value);
    let mut current = 0usize;
    for r in stripped.chars() {
        current += rune_width(r);
    }
    let mut out = stripped;
    while current < target {
        out.push(' ');
        current += 1;
    }
    str_obj(out)
}

pub(crate) fn text_wrap_width(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match required_string(ctx, "text.wrapWidth", args, 0, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let width = match required_number(ctx, "text.wrapWidth", args, 1, "width") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let limit = if width <= 0.0 {
        return array(vec![str_obj(String::new())]);
    } else {
        width as usize
    };
    let stripped = strip_ansi(&value);
    let mut lines: Vec<Object> = Vec::new();
    for raw_line in stripped.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let mut current = String::new();
        let mut used = 0usize;
        for r in line.chars() {
            let w = rune_width(r);
            if used + w > limit && !current.is_empty() {
                lines.push(str_obj(current.clone()));
                current.clear();
                used = 0;
            }
            current.push(r);
            used += w;
        }
        lines.push(str_obj(current));
    }
    array(lines)
}

pub(crate) fn text_strip_ansi(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match required_string(ctx, "text.stripAnsi", args, 0, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    str_obj(strip_ansi(&value))
}

// ---------------------------------------------------------------------------
// cli: command/flag parsing subset backed by closure-owned state.
// ---------------------------------------------------------------------------
