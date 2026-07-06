use std::cell::RefCell;
use std::rc::Rc;

use crate::object::*;

use super::super::string_lit::unescape_string;
use super::native;

pub(super) fn json_object() -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    {
        let mut h = hash.borrow_mut();
        h.set(
            "stringify",
            native("JSON.stringify", |_ctx, args| match args.first() {
                // JSON.stringify(value) -> compact (single-line), matching JS.
                // JSON.stringify(value, null, space) -> pretty when space > 0.
                Some(o) => {
                    let space = match args.get(2) {
                        Some(Object::Number(n)) if *n > 0.0 => Some(*n as usize),
                        _ => None,
                    };
                    match space {
                        Some(n) => str_obj(json_stringify_pretty(o, 0, n)),
                        None => str_obj(json_stringify_compact(o)),
                    }
                }
                None => str_obj("undefined"),
            }),
        );
        h.set(
            "parse",
            native("JSON.parse", |_ctx, args| match args.first() {
                Some(Object::String(s)) => match json_parse(s) {
                    Ok(v) => v,
                    Err(e) => new_error(
                        crate::ast::Position::default(),
                        format!("SyntaxError: {}", e),
                    ),
                },
                _ => new_error(
                    crate::ast::Position::default(),
                    "TypeError: JSON.parse requires a string",
                ),
            }),
        );
    }
    Object::Hash(hash)
}

fn json_stringify_compact(obj: &Object) -> String {
    match obj {
        Object::Number(n) => format_number(*n),
        Object::String(s) => format!("{:?}", s.as_str()),
        Object::Boolean(b) => b.to_string(),
        Object::Null => "null".into(),
        Object::Undefined => "null".into(),
        Object::Array(a) => {
            let items: Vec<String> = a
                .borrow()
                .elements
                .iter()
                .map(json_stringify_compact)
                .collect();
            format!("[{}]", items.join(","))
        }
        Object::Hash(h) => {
            let items: Vec<String> = h
                .borrow()
                .entries
                .iter()
                .map(|(k, v)| format!("{:?}: {}", k, json_stringify_compact(v)))
                .collect();
            format!("{{{}}}", items.join(","))
        }
        _ => "null".into(),
    }
}

fn json_stringify_pretty(obj: &Object, indent: usize, space: usize) -> String {
    let pad_str = " ".repeat(space);
    match obj {
        Object::Number(n) => format_number(*n),
        Object::String(s) => format!("{:?}", s.as_str()),
        Object::Boolean(b) => b.to_string(),
        Object::Null => "null".into(),
        Object::Undefined => "null".into(),
        Object::Array(a) => {
            let pad = pad_str.repeat(indent);
            let inner = pad_str.repeat(indent + 1);
            let items: Vec<String> = a
                .borrow_mut()
                .elements
                .iter()
                .map(|e| format!("{}{}", inner, json_stringify_pretty(e, indent + 1, space)))
                .collect();
            if items.is_empty() {
                "[]".into()
            } else {
                format!("[\n{}\n{}]", items.join(",\n"), pad)
            }
        }
        Object::Hash(h) => {
            let pad = pad_str.repeat(indent);
            let inner = pad_str.repeat(indent + 1);
            let items: Vec<String> = h
                .borrow_mut()
                .entries
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{}{:?}: {}",
                        inner,
                        k,
                        json_stringify_pretty(v, indent + 1, space)
                    )
                })
                .collect();
            if items.is_empty() {
                "{}".into()
            } else {
                format!("{{\n{}\n{}}}", items.join(",\n"), pad)
            }
        }
        _ => "null".into(),
    }
}

fn json_parse(s: &str) -> Result<Object, String> {
    let mut chars = s.chars().peekable();
    skip_ws(&mut chars);
    let v = parse_json_value(&mut chars)?;
    skip_ws(&mut chars);
    Ok(v)
}

fn skip_ws(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while let Some(c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
}

fn parse_json_value(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Object, String> {
    skip_ws(chars);
    match chars.peek() {
        Some('{') => parse_json_object(chars),
        Some('[') => parse_json_array(chars),
        Some('"') => Ok(str_obj(parse_json_string(chars)?)),
        Some('t') | Some('f') => parse_json_bool(chars),
        Some('n') => {
            consume(chars, "null")?;
            Ok(Object::Null)
        }
        Some(c) if c.is_ascii_digit() || *c == '-' => parse_json_number(chars),
        _ => Err("unexpected token".into()),
    }
}

fn parse_json_object(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Object, String> {
    chars.next(); // {
    let hash = Rc::new(RefCell::new(HashData::default()));
    skip_ws(chars);
    if chars.peek() == Some(&'}') {
        chars.next();
        return Ok(Object::Hash(hash));
    }
    loop {
        skip_ws(chars);
        let key = parse_json_string(chars)?;
        skip_ws(chars);
        if chars.next() != Some(':') {
            return Err("expected :".into());
        }
        let val = parse_json_value(chars)?;
        hash.borrow_mut().set(key, val);
        skip_ws(chars);
        match chars.next() {
            Some(',') => continue,
            Some('}') => break,
            _ => return Err("expected , or }".into()),
        }
    }
    Ok(Object::Hash(hash))
}

fn parse_json_array(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Object, String> {
    chars.next(); // [
    let mut elems = Vec::new();
    skip_ws(chars);
    if chars.peek() == Some(&']') {
        chars.next();
        return Ok(Object::Array(Rc::new(RefCell::new(ArrayData {
            elements: elems,
        }))));
    }
    loop {
        let v = parse_json_value(chars)?;
        elems.push(v);
        skip_ws(chars);
        match chars.next() {
            Some(',') => continue,
            Some(']') => break,
            _ => return Err("expected , or ]".into()),
        }
    }
    Ok(Object::Array(Rc::new(RefCell::new(ArrayData {
        elements: elems,
    }))))
}

fn parse_json_string(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<String, String> {
    if chars.next() != Some('"') {
        return Err("expected string".into());
    }
    let mut raw = String::new();
    while let Some(c) = chars.next() {
        if c == '"' {
            return Ok(unescape_string(&raw));
        }
        if c == '\\' {
            if let Some(e) = chars.next() {
                raw.push('\\');
                raw.push(e);
            }
        } else {
            raw.push(c);
        }
    }
    Err("unterminated string".into())
}

fn parse_json_bool(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Object, String> {
    if chars.peek() == Some(&'t') {
        consume(chars, "true")?;
        Ok(Object::Boolean(true))
    } else {
        consume(chars, "false")?;
        Ok(Object::Boolean(false))
    }
}

fn parse_json_number(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Object, String> {
    let mut s = String::new();
    while let Some(c) = chars.peek() {
        if c.is_ascii_digit() || *c == '-' || *c == '+' || *c == '.' || *c == 'e' || *c == 'E' {
            s.push(*c);
            chars.next();
        } else {
            break;
        }
    }
    s.parse::<f64>()
        .map(Object::Number)
        .map_err(|e| e.to_string())
}

fn consume(chars: &mut std::iter::Peekable<std::str::Chars>, lit: &str) -> Result<(), String> {
    for c in lit.chars() {
        if chars.next() != Some(c) {
            return Err(format!("expected {}", lit));
        }
    }
    Ok(())
}
