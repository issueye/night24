use std::fs;

use super::super::helpers::*;
use crate::object::{new_error, str_obj, CallContext, Object};

pub(crate) fn template_module() -> Object {
    module(vec![
        ("render", native("template.render", template_render)),
        (
            "renderHTML",
            native("template.renderHTML", template_render_html),
        ),
        (
            "renderFileSync",
            native("template.renderFileSync", template_render_file_sync),
        ),
        (
            "escapeHTML",
            native("template.escapeHTML", template_escape_html),
        ),
    ])
}

pub(crate) fn template_render(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "template.render", args);
    let source = match reader.required_string(0, "source") {
        Ok(source) => source,
        Err(err) => return err,
    };
    template_execute(&source, args.get(1).unwrap_or(&Object::Undefined), false)
        .map(str_obj)
        .unwrap_or_else(|e| new_error(ctx.pos.clone(), format!("template.render: {}", e)))
}

pub(crate) fn template_render_html(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "template.renderHTML", args);
    let source = match reader.required_string(0, "source") {
        Ok(source) => source,
        Err(err) => return err,
    };
    template_execute(&source, args.get(1).unwrap_or(&Object::Undefined), true)
        .map(str_obj)
        .unwrap_or_else(|e| new_error(ctx.pos.clone(), format!("template.renderHTML: {}", e)))
}

pub(crate) fn template_render_file_sync(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "template.renderFileSync", args);
    let path = match reader.required_string(0, "path") {
        Ok(path) => path,
        Err(err) => return err,
    };
    match fs::read_to_string(&path) {
        Ok(source) => template_execute(&source, args.get(1).unwrap_or(&Object::Undefined), false)
            .map(str_obj)
            .unwrap_or_else(|e| {
                new_error(ctx.pos.clone(), format!("template.renderFileSync: {}", e))
            }),
        Err(e) => new_error(ctx.pos.clone(), format!("template.renderFileSync: {}", e)),
    }
}

pub(crate) fn template_escape_html(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "template.escapeHTML", args);
    match reader.required_string(0, "value") {
        Ok(value) => str_obj(escape_html(&value)),
        Err(err) => err,
    }
}

pub(crate) fn template_execute(source: &str, data: &Object, html: bool) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = source;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return Err("unterminated action".into());
        };
        let expr = after[..end].trim();
        let mut text = template_eval_expr(expr, data)?;
        if html {
            text = escape_html(&text);
        }
        out.push_str(&text);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

pub(crate) fn template_eval_expr(expr: &str, data: &Object) -> Result<String, String> {
    let parts = split_template_args(expr);
    if parts.is_empty() {
        return Ok(String::new());
    }
    match parts[0].as_str() {
        "upper" => Ok(template_value_text(parts.get(1), data)?.to_uppercase()),
        "lower" => Ok(template_value_text(parts.get(1), data)?.to_lowercase()),
        "trim" => Ok(template_value_text(parts.get(1), data)?.trim().to_string()),
        "join" => {
            let value = template_lookup(parts.get(1).map(String::as_str).unwrap_or("."), data)?;
            let sep = parts
                .get(2)
                .map(|s| unquote_template_arg(s))
                .unwrap_or_default();
            match value {
                Object::Array(arr) => Ok(arr
                    .borrow()
                    .elements
                    .iter()
                    .map(object_to_text)
                    .collect::<Vec<_>>()
                    .join(&sep)),
                other => Ok(object_to_text(&other)),
            }
        }
        "json" => {
            let value = template_lookup(parts.get(1).map(String::as_str).unwrap_or("."), data)?;
            Ok(object_to_json(&value, 0, None))
        }
        _ => template_lookup(expr, data).map(|value| object_to_text(&value)),
    }
}

pub(crate) fn template_value_text(token: Option<&String>, data: &Object) -> Result<String, String> {
    let Some(token) = token else {
        return Ok(String::new());
    };
    if token.starts_with('.') {
        template_lookup(token, data).map(|value| object_to_text(&value))
    } else {
        Ok(unquote_template_arg(token))
    }
}

pub(crate) fn split_template_args(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut quote = '\0';
    for ch in expr.chars() {
        if in_quote {
            if ch == quote {
                in_quote = false;
            }
            current.push(ch);
        } else if ch == '"' || ch == '\'' {
            in_quote = true;
            quote = ch;
            current.push(ch);
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                out.push(current.clone());
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

pub(crate) fn unquote_template_arg(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(value)
        .to_string()
}

pub(crate) fn template_lookup(expr: &str, data: &Object) -> Result<Object, String> {
    if expr == "." {
        return Ok(data.clone());
    }
    let path = expr
        .strip_prefix('.')
        .ok_or_else(|| format!("unsupported action {}", expr))?;
    let mut current = data.clone();
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        match current {
            Object::Hash(hash) => {
                current = hash
                    .borrow()
                    .get(segment)
                    .cloned()
                    .unwrap_or(Object::Undefined);
            }
            _ => return Ok(Object::Undefined),
        }
    }
    Ok(current)
}

pub(crate) fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&#34;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_html_escapes_reserved_characters() {
        assert_eq!(
            escape_html("<a href=\"/x?y=1&z='2'\">Night24</a>"),
            "&lt;a href=&#34;/x?y=1&amp;z=&#39;2&#39;&#34;&gt;Night24&lt;/a&gt;"
        );
    }

    #[test]
    fn template_execute_renders_basic_lookup() {
        let data = module(vec![("name", str_obj("Night24"))]);

        assert_eq!(
            template_execute("Hello, {{ .name }}!", &data, false).unwrap(),
            "Hello, Night24!"
        );
    }
}

// ---------------------------------------------------------------------------
// compression / compress/gzip: gzip round-trips using the shared buffer shape.
// ---------------------------------------------------------------------------
