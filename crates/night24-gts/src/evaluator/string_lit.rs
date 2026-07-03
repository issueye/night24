//! String, template, and regexp literal evaluation (with escape processing).

use std::rc::Rc;

use crate::ast::*;
use crate::lexer::Lexer;
use crate::object::*;
use crate::parser::Parser;

/// Evaluate a string literal (process escapes).
pub fn eval_string_lit(s: &StringLit) -> Object {
    let lit = &s.literal;
    if lit.len() < 2 {
        return str_obj("");
    }
    let inner = &lit[1..lit.len() - 1];
    str_obj(unescape_string(inner))
}

/// Evaluate a template literal that contains no `${...}` interpolation.
///
/// This is the compile-time-reducible subset used by the bytecode compiler.
/// The result is identical to what `eval_template` would produce for the
/// same literal (same escape handling), just without needing an environment.
pub fn eval_template_static(t: &TemplateLit) -> Object {
    let lit = &t.literal;
    if lit.len() < 2 || !lit.starts_with('`') {
        return str_obj(lit.clone());
    }
    let mut inner = &lit[1..];
    if inner.ends_with('`') {
        inner = &inner[..inner.len() - 1];
    }
    str_obj(unescape_string(inner))
}

/// Evaluate a template literal, interpolating `${...}` expressions.
pub fn eval_template(t: &TemplateLit, env: &EnvRef) -> Object {
    let lit = &t.literal;
    if lit.len() < 2 || !lit.starts_with('`') {
        return str_obj(lit.clone());
    }
    let mut inner = &lit[1..];
    if inner.ends_with('`') {
        inner = &inner[..inner.len() - 1];
    }
    let bytes = inner.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            let end = find_template_expr_end(inner, i + 2);
            if let Some(end) = end {
                let expr_str = inner[i + 2..end].trim();
                if !expr_str.is_empty() {
                    let val = eval_template_expression(expr_str, env, t.pos.clone());
                    if val.is_runtime_error() {
                        return val;
                    }
                    out.push_str(&val.inspect());
                }
                i = end + 1;
                continue;
            } else {
                return new_error(
                    t.pos.clone(),
                    "SyntaxError: unterminated template expression",
                );
            }
        }
        // collect a run of literal chars
        let start = i;
        while i < bytes.len() && !(i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{')
        {
            i += 1;
        }
        out.push_str(&unescape_string(&inner[start..i]));
    }
    str_obj(out)
}

fn find_template_expr_end(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut quote: u8 = 0;
    let mut escape = false;
    let mut i = start;
    while i < bytes.len() {
        let ch = bytes[i];
        if quote != 0 {
            if escape {
                escape = false;
            } else if ch == b'\\' {
                escape = true;
            } else if ch == quote {
                quote = 0;
            }
            i += 1;
            continue;
        }
        match ch {
            b'"' | b'\'' => quote = ch,
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn eval_template_expression(expr: &str, env: &EnvRef, pos: Position) -> Object {
    let src = format!("let __gts_template_expr = {};", expr);
    let lex = Lexer::new(&src);
    let mut parser = Parser::new(lex, pos.file.as_ref());
    let prog = parser.parse_program();
    if !parser.errors().is_empty() || !prog.errors.is_empty() {
        return new_error(
            pos,
            "SyntaxError: template expression parse error".to_string(),
        );
    }
    let scope = Environment::child(env);
    let r = crate::evaluator::eval_core::eval_program(&prog, &scope);
    if r.is_runtime_error() {
        return r;
    }
    let value = scope
        .borrow_mut()
        .get("__gts_template_expr")
        .unwrap_or(Object::Undefined);
    value
}

/// Evaluate a regexp literal.
pub fn eval_regexp_lit(r: &RegExpLit) -> Object {
    let (source, flags) = split_regexp(&r.literal);
    let pattern = if flags.contains('i') {
        format!("(?i){}", source)
    } else {
        source.clone()
    };
    match regex::Regex::new(&pattern) {
        Ok(re) => Object::Regexp(Rc::new(RegexpData { source, flags, re })),
        Err(_) => new_error(
            r.pos.clone(),
            format!("SyntaxError: invalid regexp /{}/{}", source, flags),
        ),
    }
}

fn split_regexp(lit: &str) -> (String, String) {
    let b = lit.as_bytes();
    if b.len() < 2 || b[0] != b'/' {
        return (String::new(), String::new());
    }
    let mut in_class = false;
    let mut escape = false;
    for (i, &ch) in b.iter().enumerate().skip(1) {
        if escape {
            escape = false;
            continue;
        }
        if ch == b'\\' {
            escape = true;
            continue;
        }
        if in_class {
            if ch == b']' {
                in_class = false;
            }
            continue;
        }
        if ch == b'[' {
            in_class = true;
            continue;
        }
        if ch == b'/' {
            return (lit[1..i].to_string(), lit[i + 1..].to_string());
        }
    }
    (String::new(), String::new())
}

/// Process string escape sequences.
pub fn unescape_string(s: &str) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            match next {
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                b'r' => out.push('\r'),
                b'b' => out.push('\u{0008}'),
                b'f' => out.push('\u{000C}'),
                b'v' => out.push('\u{000B}'),
                b'0' => out.push('\0'),
                b'\\' => out.push('\\'),
                b'"' => out.push('"'),
                b'\'' => out.push('\''),
                b'`' => out.push('`'),
                b'x' => {
                    if i + 3 < bytes.len() && is_hex(bytes[i + 2]) && is_hex(bytes[i + 3]) {
                        let hex = &s[i + 2..i + 4];
                        if let Ok(v) = u32::from_str_radix(hex, 16) {
                            out.push(v as u8 as char);
                        }
                        i += 4;
                        continue;
                    }
                    out.push('x');
                }
                b'u' => {
                    if i + 5 < bytes.len()
                        && is_hex(bytes[i + 2])
                        && is_hex(bytes[i + 3])
                        && is_hex(bytes[i + 4])
                        && is_hex(bytes[i + 5])
                    {
                        let hex = &s[i + 2..i + 6];
                        if let Ok(v) = u32::from_str_radix(hex, 16) {
                            if let Some(c) = char::from_u32(v) {
                                out.push(c);
                            }
                        }
                        i += 6;
                        continue;
                    }
                    if i + 3 < bytes.len() && bytes[i + 2] == b'{' {
                        if let Some(end) = s[i + 3..].find('}') {
                            let hex = &s[i + 3..i + 3 + end];
                            if !hex.is_empty() && hex.bytes().all(is_hex) {
                                if let Ok(v) = u32::from_str_radix(hex, 16) {
                                    if let Some(c) = char::from_u32(v) {
                                        out.push(c);
                                        i += 4 + end;
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    out.push('u');
                }
                other => out.push(other as char),
            }
            i += 2;
        } else {
            // Copy a multibyte char safely.
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || (b'A'..=b'F').contains(&b)
}
