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
    let reader = ArgReader::new(ctx, "text.chars", args);
    let value = match reader.required_string(0, "value") {
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
    let reader = ArgReader::new(ctx, "text.width", args);
    let value = match reader.required_string(0, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    num_obj(visible_width(&value) as f64)
}

pub(crate) fn text_truncate_width(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "text.truncateWidth", args);
    let value = match reader.required_string(0, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let width = match reader.required_number(1, "width") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let limit = if width < 0.0 { 0 } else { width as usize };
    str_obj(truncate_visible_width(&value, limit))
}

pub(crate) fn text_pad_right_width(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "text.padRightWidth", args);
    let value = match reader.required_string(0, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let width = match reader.required_number(1, "width") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let target = if width < 0.0 { 0 } else { width as usize };
    let stripped = strip_ansi(&value);
    let mut current = visible_width(&stripped);
    let mut out = stripped;
    while current < target {
        out.push(' ');
        current += 1;
    }
    str_obj(out)
}

pub(crate) fn text_wrap_width(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "text.wrapWidth", args);
    let value = match reader.required_string(0, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let width = match reader.required_number(1, "width") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let limit = if width <= 0.0 {
        return array(vec![str_obj(String::new())]);
    } else {
        width as usize
    };
    array(
        wrap_visible_width(&value, limit)
            .into_iter()
            .map(str_obj)
            .collect(),
    )
}

pub(crate) fn text_strip_ansi(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "text.stripAnsi", args);
    let value = match reader.required_string(0, "value") {
        Ok(v) => v,
        Err(e) => return e,
    };
    str_obj(strip_ansi(&value))
}

fn truncate_visible_width(value: &str, limit: usize) -> String {
    let stripped = strip_ansi(value);
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
    out
}

fn wrap_visible_width(value: &str, limit: usize) -> Vec<String> {
    let stripped = strip_ansi(value);
    let mut lines = Vec::new();
    for raw_line in stripped.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let mut current = String::new();
        let mut used = 0usize;
        for r in line.chars() {
            let w = rune_width(r);
            if used + w > limit && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                used = 0;
            }
            current.push(r);
            used += w;
        }
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_width_ignores_ansi_sequences_and_counts_wide_runes() {
        assert_eq!(visible_width("\x1b[31mHi\x1b[0m 世界"), 7);
    }

    #[test]
    fn truncate_width_strips_ansi_before_applying_display_limit() {
        assert_eq!(truncate_visible_width("\x1b[32mA界B\x1b[0m", 3), "A界");
    }

    #[test]
    fn wrap_width_strips_ansi_and_preserves_input_line_breaks() {
        assert_eq!(
            wrap_visible_width("\x1b[31mAB界C\x1b[0m\r\nD", 3),
            vec!["AB".to_string(), "界C".to_string(), "D".to_string()]
        );
    }
}

// ---------------------------------------------------------------------------
// cli: command/flag parsing subset backed by closure-owned state.
// ---------------------------------------------------------------------------
