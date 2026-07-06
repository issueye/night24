use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{num_obj, str_obj, CallContext, Object};

pub(crate) fn markdown_module() -> Object {
    module(vec![
        ("parse", native("markdown.parse", markdown_parse)),
        (
            "renderTerminal",
            native("markdown.renderTerminal", markdown_render_terminal),
        ),
        ("fromHTML", native("markdown.fromHTML", markdown_from_html)),
        (
            "createStream",
            native("markdown.createStream", markdown_create_stream),
        ),
    ])
}

pub(crate) fn markdown_parse(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "markdown.parse", args);
    let source = match reader.required_string(0, "source") {
        Ok(v) => v,
        Err(e) => return e,
    };
    str_obj(source)
}

pub(crate) fn markdown_render_terminal(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "markdown.renderTerminal", args);
    let source = match reader.required_string(0, "source") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let width = reader
        .object_view(1)
        .and_then(|opts| {
            ObjectView::new(&opts)
                .number("width")
                .filter(|value| *value >= 1.0)
                .map(|value| value as usize)
        })
        .unwrap_or(80);
    let normalized: String = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let mut out_lines: Vec<Object> = Vec::new();
    let mut headings: Vec<Object> = Vec::new();
    for line in &lines {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            out_lines.push(str_obj(format!("# {}", rest.trim())));
            headings.push(str_obj(rest.trim().to_string()));
        } else if let Some(rest) = trimmed.strip_prefix("```") {
            out_lines.push(str_obj(format!("  {}", rest)));
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            out_lines.push(str_obj(format!("- {}", &trimmed[2..])));
        } else if trimmed == "---" || trimmed == "***" {
            out_lines.push(str_obj("-".repeat(width)));
        } else if !trimmed.is_empty() {
            out_lines.push(str_obj(trimmed.to_string()));
        }
    }
    ObjectBuilder::new()
        .set("lines", array(out_lines))
        .set("width", num_obj(width as f64))
        .set("headings", array(headings))
        .set("links", array(Vec::new()))
        .build()
}

pub(crate) fn markdown_create_stream(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "markdown.createStream", args);
    let source = match reader.required_string(0, "source") {
        Ok(v) => v,
        Err(e) => return e,
    };
    build_markdown_token_stream(source)
}

/// A parsed markdown token: `{ type, text }`.
/// Types: "heading" (text includes the heading content), "paragraph",
/// "list_item", "code_fence", "hr". EOF is signalled by `next()` returning null.
fn md_token(kind: &str, text: &str) -> Object {
    ObjectBuilder::new()
        .set("type", str_obj(kind))
        .set("text", str_obj(text))
        .build()
}

/// Build a streaming token reader over markdown source.
///
/// The VM is synchronous, so "streaming" here means incremental consumption:
/// `createStream(text)` returns a reader with `.next()` (yields the next token
/// or null at EOF), `.tokens()` (all remaining tokens), `.headings()` (heading
/// texts only), and `.index`/`.count`. This lets callers process a document
/// token-by-token without buffering the whole structure.
pub(crate) fn build_markdown_token_stream(source: String) -> Object {
    let normalized: String = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut tokens: Vec<Object> = Vec::new();
    let mut in_fence = false;
    for line in normalized.lines() {
        let trimmed = line.trim_start();
        if in_fence {
            if trimmed.starts_with("```") {
                in_fence = false;
            }
            tokens.push(md_token("code_fence", line));
            continue;
        }
        if trimmed.starts_with("```") {
            in_fence = true;
            tokens.push(md_token("code_fence", line));
        } else if let Some(rest) = trimmed
            .strip_prefix("# ")
            .or_else(|| trimmed.strip_prefix("## "))
            .or_else(|| trimmed.strip_prefix("### "))
        {
            tokens.push(md_token("heading", rest.trim()));
        } else if trimmed == "---" || trimmed == "***" {
            tokens.push(md_token("hr", ""));
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            tokens.push(md_token("list_item", trimmed[2..].trim()));
        } else if !trimmed.is_empty() {
            tokens.push(md_token("paragraph", trimmed));
        }
    }

    let headings: Vec<Object> = tokens
        .iter()
        .filter_map(|t| match t {
            Object::Hash(h) => {
                let b = h.borrow();
                if let (Some(Object::String(k)), Some(Object::String(txt))) =
                    (b.get("type"), b.get("text"))
                {
                    if k.as_str() == "heading" {
                        return Some(str_obj(txt.to_string()));
                    }
                }
                None
            }
            _ => None,
        })
        .collect();

    let tokens_obj: Rc<RefCell<Vec<Object>>> = Rc::new(RefCell::new(tokens));
    let cursor: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));

    let reader = ObjectBuilder::new()
        .set("count", num_obj(tokens_obj.borrow().len() as f64))
        .into_shared();

    // .index — live cursor position (re-read each access via a getter closure).
    {
        let cursor = cursor.clone();
        let idx_obj = native("markdown.createStream.index", move |_ctx, _args| {
            num_obj(*cursor.borrow() as f64)
        });
        reader.borrow_mut().set("index", idx_obj);
    }
    // .next() -> token or null at EOF
    {
        let tokens = tokens_obj.clone();
        let cursor = cursor.clone();
        let next_obj = native("markdown.createStream.next", move |_ctx, _args| {
            let mut pos = cursor.borrow_mut();
            let tokens = tokens.borrow();
            if *pos >= tokens.len() {
                Object::Null
            } else {
                let tok = tokens[*pos].clone();
                *pos += 1;
                tok
            }
        });
        reader.borrow_mut().set("next", next_obj);
    }
    // .tokens() -> all remaining tokens (does not advance cursor)
    {
        let tokens = tokens_obj.clone();
        let cursor = cursor.clone();
        let all_obj = native("markdown.createStream.tokens", move |_ctx, _args| {
            let pos = *cursor.borrow();
            let tokens = tokens.borrow();
            array(tokens[pos..].to_vec())
        });
        reader.borrow_mut().set("tokens", all_obj);
    }
    // .headings() -> heading texts only
    {
        let headings = headings.clone();
        let head_obj = native("markdown.createStream.headings", move |_ctx, _args| {
            array(headings.clone())
        });
        reader.borrow_mut().set("headings", head_obj);
    }
    // .reset() -> rewind cursor to 0
    {
        let cursor = cursor.clone();
        let reset_obj = native("markdown.createStream.reset", move |_ctx, _args| {
            *cursor.borrow_mut() = 0;
            Object::Undefined
        });
        reader.borrow_mut().set("reset", reset_obj);
    }

    Object::Hash(reader)
}

pub(crate) fn markdown_from_html(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "markdown.fromHTML", args);
    let html = match reader.required_string(0, "html") {
        Ok(v) => v,
        Err(e) => return e,
    };
    str_obj(html_to_markdown(&html))
}

/// Minimal HTML-to-markdown: strip tags, preserve text, convert a few common
/// block elements to markdown equivalents.
fn html_to_markdown(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut in_tag = false;
    let mut tag = String::new();
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'<' {
            in_tag = true;
            tag.clear();
            i += 1;
            continue;
        }
        if c == b'>' {
            in_tag = false;
            let lower = tag.trim().to_lowercase();
            match lower.as_str() {
                "h1" | "h2" | "h3" => out.push_str("\n# "),
                "li" => out.push_str("\n- "),
                "p" | "br" | "div" => out.push('\n'),
                _ => {}
            }
            i += 1;
            continue;
        }
        if in_tag {
            tag.push(c as char);
        } else {
            out.push(c as char);
        }
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// schema: JSON-Schema-style validate/assert.
// ---------------------------------------------------------------------------
