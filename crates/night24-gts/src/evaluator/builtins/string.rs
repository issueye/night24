use std::cell::RefCell;
use std::rc::Rc;

use crate::object::*;

use super::{as_num, normalize_index, FnPtr};

pub(super) fn string_global() -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    let call_fn: FnPtr = Rc::new(|_ctx, args| match args.first() {
        Some(o) => str_obj(o.inspect()),
        None => str_obj(""),
    });
    hash.borrow_mut().set(
        "__call",
        Object::Builtin(Rc::new(Builtin {
            name: "String".into(),
            func: call_fn,
            extra: None,
        })),
    );
    Object::Hash(hash)
}

pub fn string_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "toUpperCase" => Some(str_upper),
        "toLowerCase" => Some(str_lower),
        "trim" => Some(str_trim),
        "trimStart" => Some(str_trim_start),
        "trimEnd" => Some(str_trim_end),
        "split" => Some(str_split),
        "replace" => Some(str_replace),
        "replaceAll" => Some(str_replace_all),
        "includes" => Some(str_includes),
        "startsWith" => Some(str_starts_with),
        "endsWith" => Some(str_ends_with),
        "indexOf" => Some(str_index_of),
        "lastIndexOf" => Some(str_last_index_of),
        "slice" => Some(str_slice),
        "substring" => Some(str_substring),
        "charAt" => Some(str_char_at),
        "repeat" => Some(str_repeat),
        "padStart" => Some(str_pad_start),
        "padEnd" => Some(str_pad_end),
        "concat" => Some(str_concat),
        "match" => Some(str_match),
        "search" => Some(str_search),
        "localeCompare" => Some(str_locale_compare),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}

fn active_string(ctx: &CallContext) -> Option<Rc<String>> {
    match &ctx.receiver {
        Some(Object::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn str_upper(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        return str_obj(s.to_uppercase());
    }
    str_obj("")
}
fn str_lower(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        return str_obj(s.to_lowercase());
    }
    str_obj("")
}
fn str_trim(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        return str_obj(s.trim().to_string());
    }
    str_obj("")
}
fn str_trim_start(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        return str_obj(s.trim_start().to_string());
    }
    str_obj("")
}
fn str_trim_end(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        return str_obj(s.trim_end().to_string());
    }
    str_obj("")
}
fn str_split(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        let parts: Vec<Object> = match args.first() {
            Some(Object::String(sep)) => {
                if sep.is_empty() {
                    s.chars().map(|c| str_obj(c.to_string())).collect()
                } else {
                    s.split(sep.as_str())
                        .map(|p| str_obj(p.to_string()))
                        .collect()
                }
            }
            _ => vec![str_obj(s.to_string())],
        };
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: parts })));
    }
    Object::Undefined
}
fn str_replace(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        let to = match args.get(1) {
            Some(Object::String(x)) => x.to_string(),
            _ => String::new(),
        };
        if let Some(Object::Regexp(re)) = args.first() {
            let replaced = if re.flags.contains('g') {
                re.re.replace_all(s.as_str(), to.as_str()).into_owned()
            } else {
                re.re.replace(s.as_str(), to.as_str()).into_owned()
            };
            return str_obj(replaced);
        }
        let from = match args.first() {
            Some(Object::String(x)) => x.to_string(),
            _ => return str_obj(s.to_string()),
        };
        return str_obj(s.replacen(&from, &to, 1));
    }
    str_obj("")
}
fn str_replace_all(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        let to = match args.get(1) {
            Some(Object::String(x)) => x.to_string(),
            _ => String::new(),
        };
        if let Some(Object::Regexp(re)) = args.first() {
            return str_obj(re.re.replace_all(s.as_str(), to.as_str()).into_owned());
        }
        let from = match args.first() {
            Some(Object::String(x)) => x.to_string(),
            _ => return str_obj(s.to_string()),
        };
        return str_obj(s.replace(&from, &to));
    }
    str_obj("")
}
fn str_includes(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        if let Some(Object::String(needle)) = args.first() {
            return Object::Boolean(s.contains(needle.as_str()));
        }
    }
    Object::Boolean(false)
}
fn str_starts_with(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        if let Some(Object::String(p)) = args.first() {
            return Object::Boolean(s.starts_with(p.as_str()));
        }
    }
    Object::Boolean(false)
}
fn str_ends_with(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        if let Some(Object::String(p)) = args.first() {
            return Object::Boolean(s.ends_with(p.as_str()));
        }
    }
    Object::Boolean(false)
}
fn str_index_of(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        if let Some(Object::String(needle)) = args.first() {
            if let Some(idx) = s.find(needle.as_str()) {
                return Object::Number(s[..idx].chars().count() as f64);
            }
        }
    }
    Object::Number(-1.0)
}
fn str_last_index_of(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        if let Some(Object::String(needle)) = args.first() {
            if needle.is_empty() {
                return Object::Number(s.chars().count() as f64);
            }
            if let Some(idx) = s.rfind(needle.as_str()) {
                return Object::Number(s[..idx].chars().count() as f64);
            }
        }
    }
    Object::Number(-1.0)
}
fn str_slice(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        let chars: Vec<char> = s.chars().collect();
        let len = chars.len() as isize;
        let start = normalize_index(as_num(args.first()) as isize, len);
        let end = match args.get(1) {
            Some(Object::Number(n)) => normalize_index(*n as isize, len),
            _ => len,
        };
        let s2 = start.max(0) as usize;
        let e2 = end.max(0).min(len) as usize;
        if s2 < e2 && s2 <= chars.len() {
            let out: String = chars[s2..e2.min(chars.len())].iter().collect();
            return str_obj(out);
        }
        return str_obj("");
    }
    str_obj("")
}
fn str_substring(ctx: &mut CallContext, args: &[Object]) -> Object {
    str_slice(ctx, args)
}
fn str_char_at(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        if let Some(Object::Number(n)) = args.first() {
            if let Some(c) = s.chars().nth(*n as usize) {
                return str_obj(c.to_string());
            }
        }
    }
    str_obj("")
}
fn str_repeat(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        let n = as_num(args.first()) as usize;
        return str_obj(s.repeat(n));
    }
    str_obj("")
}
fn str_pad_start(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        let target = as_num(args.first()) as usize;
        let pad = match args.get(1) {
            Some(Object::String(p)) => p.to_string(),
            _ => " ".into(),
        };
        let len = s.chars().count();
        if len >= target || pad.is_empty() {
            return str_obj(s.to_string());
        }
        let need = target - len;
        let pad_chars: Vec<char> = pad.chars().collect();
        let mut out = String::new();
        for i in 0..need {
            out.push(pad_chars[i % pad_chars.len()]);
        }
        out.push_str(s.as_str());
        return str_obj(out);
    }
    str_obj("")
}
fn str_pad_end(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        let target = as_num(args.first()) as usize;
        let pad = match args.get(1) {
            Some(Object::String(p)) => p.to_string(),
            _ => " ".into(),
        };
        let len = s.chars().count();
        if len >= target || pad.is_empty() {
            return str_obj(s.to_string());
        }
        let need = target - len;
        let pad_chars: Vec<char> = pad.chars().collect();
        let mut out = s.to_string();
        for i in 0..need {
            out.push(pad_chars[i % pad_chars.len()]);
        }
        return str_obj(out);
    }
    str_obj("")
}
fn str_concat(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(s) = active_string(ctx) {
        let mut out = s.to_string();
        for a in args {
            out.push_str(&a.inspect());
        }
        return str_obj(out);
    }
    str_obj("")
}

fn str_match(ctx: &mut CallContext, args: &[Object]) -> Object {
    let s = match active_string(ctx) {
        Some(s) => s.to_string(),
        None => return Object::Null,
    };

    if args.is_empty() {
        return Object::Null;
    }

    match &args[0] {
        Object::Regexp(re) => {
            // Check if global flag is set
            if re.flags.contains('g') {
                // Global match - return all matches as array
                let matches: Vec<_> = re
                    .re
                    .find_iter(&s)
                    .map(|m| str_obj(m.as_str().to_string()))
                    .collect();
                if matches.is_empty() {
                    return Object::Null;
                }
                Object::Array(Rc::new(RefCell::new(ArrayData { elements: matches })))
            } else {
                // Non-global - return first match with captures
                if let Some(captures) = re.re.captures(&s) {
                    let mut result = Vec::new();
                    for cap in captures.iter() {
                        result.push(match cap {
                            Some(m) => str_obj(m.as_str().to_string()),
                            None => Object::Undefined,
                        });
                    }
                    return Object::Array(Rc::new(RefCell::new(ArrayData { elements: result })));
                }
                Object::Null
            }
        }
        _ => {
            // Convert argument to string pattern
            let pattern = args[0].inspect();
            if let Ok(re) = regex::Regex::new(&regex::escape(&pattern)) {
                if let Some(m) = re.find(&s) {
                    return Object::Array(Rc::new(RefCell::new(ArrayData {
                        elements: vec![str_obj(m.as_str().to_string())],
                    })));
                }
            }
            Object::Null
        }
    }
}

fn str_search(ctx: &mut CallContext, args: &[Object]) -> Object {
    let s = match active_string(ctx) {
        Some(s) => s.to_string(),
        None => return Object::Number(-1.0),
    };

    if args.is_empty() {
        return Object::Number(-1.0);
    }

    match &args[0] {
        Object::Regexp(re) => {
            if let Some(m) = re.re.find(&s) {
                return Object::Number(char_index_at_byte(&s, m.start()) as f64);
            }
            Object::Number(-1.0)
        }
        _ => {
            // Convert argument to string pattern
            let pattern = args[0].inspect();
            if let Ok(re) = regex::Regex::new(&regex::escape(&pattern)) {
                if let Some(m) = re.find(&s) {
                    return Object::Number(char_index_at_byte(&s, m.start()) as f64);
                }
            }
            Object::Number(-1.0)
        }
    }
}

fn char_index_at_byte(s: &str, byte_index: usize) -> usize {
    s[..byte_index].chars().count()
}

fn str_locale_compare(ctx: &mut CallContext, args: &[Object]) -> Object {
    let s = match active_string(ctx) {
        Some(s) => s.to_string(),
        None => return Object::Number(0.0),
    };

    if args.is_empty() {
        return Object::Number(0.0);
    }

    let other = args[0].inspect();

    // Simple lexicographic comparison (not locale-aware)
    // For a full locale-aware implementation, would need ICU or similar
    match s.cmp(&other) {
        std::cmp::Ordering::Less => Object::Number(-1.0),
        std::cmp::Ordering::Equal => Object::Number(0.0),
        std::cmp::Ordering::Greater => Object::Number(1.0),
    }
}
