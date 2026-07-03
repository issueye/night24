//! Global builtins (Math, JSON, Object, Array, etc.) and the method tables.

use std::cell::RefCell;
use std::rc::Rc;

use crate::object::*;

use super::console::console_object;
use super::expressions::apply_function;
use super::string_lit::unescape_string;

type FnPtr = BuiltinFn;

/// Register all standard globals on the VM.
pub fn register_globals(vm: &Rc<VirtualMachine>) {
    vm.set_global("console", console_object());

    let println_fn: FnPtr = Rc::new(|ctx, args| {
        let parts: Vec<String> = args.iter().map(|a| a.inspect()).collect();
        ctx.vm().push_stdout(parts.join(""));
        Object::Undefined
    });
    vm.set_global(
        "println",
        Object::Builtin(Rc::new(Builtin {
            name: "println".into(),
            func: println_fn,
            extra: None,
        })),
    );

    let print_fn: FnPtr = Rc::new(|ctx, args| {
        let text = args
            .iter()
            .map(|a| a.inspect())
            .collect::<Vec<_>>()
            .join("");
        ctx.vm().push_stdout(text);
        Object::Undefined
    });
    vm.set_global(
        "print",
        Object::Builtin(Rc::new(Builtin {
            name: "print".into(),
            func: print_fn,
            extra: None,
        })),
    );

    vm.set_global("Math", math_object());
    vm.set_global("JSON", json_object());
    vm.set_global("Object", object_global());
    vm.set_global("Array", array_global());
    vm.set_global("String", string_global());
    vm.set_global("Number", number_global());
    vm.set_global("Boolean", boolean_global());
    vm.set_global("Symbol", super::iterator::symbol_global());

    // Error constructors.
    for name in [
        "Error",
        "TypeError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
    ] {
        let n = name.to_string();
        let f: FnPtr = Rc::new(move |_ctx, args| {
            let message = args.first().map(|a| a.inspect()).unwrap_or_default();
            new_error_object(crate::ast::Position::default(), &n, message)
        });
        vm.set_global(
            name,
            Object::Builtin(Rc::new(Builtin {
                name: name.into(),
                func: f,
                extra: None,
            })),
        );
    }

    // Promise constructor.
    let promise_fn: FnPtr = Rc::new(promise_constructor);
    vm.set_global(
        "Promise",
        Object::Builtin(Rc::new(Builtin {
            name: "Promise".into(),
            func: promise_fn,
            extra: None,
        })),
    );

    // Date: callable returning the current epoch millis.
    let date_fn: FnPtr = Rc::new(|_ctx, _args| Object::Date(chrono_now_millis()));
    vm.set_global(
        "Date",
        Object::Builtin(Rc::new(Builtin {
            name: "Date".into(),
            func: date_fn,
            extra: None,
        })),
    );

    // setTimeout / clearTimeout / setInterval / sleepAsync.
    register_timers(vm);

    // Conversion functions.
    vm.set_global("parseInt", native("parseInt", builtin_parse_int));
    vm.set_global("parseFloat", native("parseFloat", builtin_parse_float));
    vm.set_global("isNaN", native("isNaN", builtin_is_nan));
    vm.set_global("isFinite", native("isFinite", builtin_is_finite));
    vm.set_global("String", string_global());
    vm.set_global("Number", number_global());
}

fn native(
    name: &str,
    func: impl Fn(&mut CallContext<'_>, &[Object]) -> Object + 'static,
) -> Object {
    Object::Builtin(Rc::new(Builtin {
        name: name.into(),
        func: Rc::new(func),
        extra: None,
    }))
}

fn math_object() -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    let mut h = hash.borrow_mut();
    h.set("PI", Object::Number(std::f64::consts::PI));
    h.set("E", Object::Number(std::f64::consts::E));
    h.set("LN2", Object::Number(std::f64::consts::LN_2));
    h.set("LN10", Object::Number(std::f64::consts::LN_10));
    h.set("SQRT2", Object::Number(std::f64::consts::SQRT_2));
    macro_rules! m1 {
        ($n:ident, $f:expr) => {
            h.set(
                stringify!($n),
                native(
                    concat!("Math.", stringify!($n)),
                    move |_ctx, args| match args.first() {
                        Some(Object::Number(x)) => Object::Number($f(*x)),
                        _ => Object::Number(f64::NAN),
                    },
                ),
            );
        };
    }
    m1!(abs, f64::abs);
    m1!(floor, f64::floor);
    m1!(ceil, f64::ceil);
    m1!(round, f64::round);
    m1!(trunc, f64::trunc);
    m1!(sqrt, f64::sqrt);
    m1!(cbrt, f64::cbrt);
    m1!(exp, f64::exp);
    m1!(log, f64::ln);
    m1!(log2, f64::log2);
    m1!(log10, f64::log10);
    m1!(sin, f64::sin);
    m1!(cos, f64::cos);
    m1!(tan, f64::tan);
    m1!(asin, f64::asin);
    m1!(acos, f64::acos);
    m1!(atan, f64::atan);
    m1!(sign, |x: f64| if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    });
    drop(h);
    let hash2 = hash.clone();
    let mut h2 = hash2.borrow_mut();
    h2.set(
        "pow",
        native("Math.pow", |_ctx, args| {
            let a = as_num(args.first());
            let b = as_num(args.get(1));
            Object::Number(a.powf(b))
        }),
    );
    h2.set(
        "max",
        native("Math.max", |_ctx, args| {
            Object::Number(
                args.iter()
                    .map(|a| as_num(Some(a)))
                    .fold(f64::NEG_INFINITY, f64::max),
            )
        }),
    );
    h2.set(
        "min",
        native("Math.min", |_ctx, args| {
            Object::Number(
                args.iter()
                    .map(|a| as_num(Some(a)))
                    .fold(f64::INFINITY, f64::min),
            )
        }),
    );
    h2.set(
        "random",
        native("Math.random", |_ctx, _args| {
            // Deterministic pseudo-random (Pi-based, matching the Go impl's quirk).
            Object::Number(std::f64::consts::PI.fract())
        }),
    );
    h2.set(
        "hypot",
        native("Math.hypot", |_ctx, args| {
            let sum: f64 = args.iter().map(|a| as_num(Some(a)).powi(2)).sum();
            Object::Number(sum.sqrt())
        }),
    );
    drop(h2);
    Object::Hash(hash)
}

fn as_num(o: Option<&Object>) -> f64 {
    match o {
        Some(Object::Number(n)) => *n,
        _ => 0.0,
    }
}

fn json_object() -> Object {
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

fn object_global() -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    {
        let mut h = hash.borrow_mut();
        h.set(
            "keys",
            native("Object.keys", |_ctx, args| {
                if let Some(Object::Hash(o)) = args.first() {
                    let keys: Vec<Object> = o
                        .borrow_mut()
                        .entries
                        .iter()
                        .map(|(k, _)| str_obj(k.clone()))
                        .collect();
                    return Object::Array(Rc::new(RefCell::new(ArrayData { elements: keys })));
                }
                if let Some(Object::Array(a)) = args.first() {
                    let keys: Vec<Object> = (0..a.borrow_mut().elements.len())
                        .map(|i| str_obj(i.to_string()))
                        .collect();
                    return Object::Array(Rc::new(RefCell::new(ArrayData { elements: keys })));
                }
                Object::Array(Rc::new(RefCell::new(ArrayData::default())))
            }),
        );
        h.set(
            "values",
            native("Object.values", |_ctx, args| {
                if let Some(Object::Hash(o)) = args.first() {
                    let vals: Vec<Object> = o
                        .borrow_mut()
                        .entries
                        .iter()
                        .map(|(_, v)| v.clone())
                        .collect();
                    return Object::Array(Rc::new(RefCell::new(ArrayData { elements: vals })));
                }
                Object::Array(Rc::new(RefCell::new(ArrayData::default())))
            }),
        );
        h.set(
            "entries",
            native("Object.entries", |_ctx, args| {
                if let Some(Object::Hash(o)) = args.first() {
                    let pairs: Vec<Object> = o
                        .borrow_mut()
                        .entries
                        .iter()
                        .map(|(k, v)| {
                            Object::Array(Rc::new(RefCell::new(ArrayData {
                                elements: vec![str_obj(k.clone()), v.clone()],
                            })))
                        })
                        .collect();
                    return Object::Array(Rc::new(RefCell::new(ArrayData { elements: pairs })));
                }
                Object::Array(Rc::new(RefCell::new(ArrayData::default())))
            }),
        );
        h.set(
            "assign",
            native("Object.assign", |_ctx, args| {
                if args.is_empty() {
                    return Object::Null;
                }
                if let Object::Hash(target) = &args[0] {
                    for src in &args[1..] {
                        if let Object::Hash(s) = src {
                            let entries = s.borrow_mut().entries.clone();
                            for (k, v) in entries {
                                target.borrow_mut().set(k, v);
                            }
                        }
                    }
                    return Object::Hash(target.clone());
                }
                Object::Null
            }),
        );
        h.set(
            "freeze",
            native("Object.freeze", |_ctx, args| {
                if let Some(Object::Hash(o)) = args.first() {
                    o.borrow_mut().frozen = true;
                }
                args.first().cloned().unwrap_or(Object::Undefined)
            }),
        );
        h.set(
            "create",
            native("Object.create", |_ctx, args| {
                let proto = args.first().cloned().unwrap_or(Object::Null);
                let hash = Rc::new(RefCell::new(HashData::default()));
                if !matches!(proto, Object::Null) {
                    hash.borrow_mut().proto = Some(proto);
                }
                Object::Hash(hash)
            }),
        );
        h.set(
            "fromEntries",
            native("Object.fromEntries", |_ctx, args| {
                let hash = Rc::new(RefCell::new(HashData::default()));
                if let Some(Object::Array(arr)) = args.first() {
                    let entries = arr.borrow().elements.clone();
                    for entry in entries {
                        if let Object::Array(pair) = entry {
                            let pair_data = pair.borrow();
                            if pair_data.elements.len() >= 2 {
                                let key = pair_data.elements[0].inspect();
                                let value = pair_data.elements[1].clone();
                                hash.borrow_mut().set(key, value);
                            }
                        }
                    }
                }
                Object::Hash(hash)
            }),
        );
    }
    // Object as a callable: Object(x) converts to object.
    let call_fn: FnPtr = Rc::new(move |_ctx, args| match args.first() {
        Some(Object::Hash(h)) => Object::Hash(h.clone()),
        _ => {
            let hash = Rc::new(RefCell::new(HashData::default()));
            Object::Hash(hash)
        }
    });
    hash.borrow_mut().set(
        "__call",
        Object::Builtin(Rc::new(Builtin {
            name: "Object".into(),
            func: call_fn,
            extra: None,
        })),
    );
    Object::Hash(hash)
}

fn array_global() -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    {
        let mut h = hash.borrow_mut();
        h.set(
            "isArray",
            native("Array.isArray", |_ctx, args| {
                Object::Boolean(matches!(args.first(), Some(Object::Array(_))))
            }),
        );
        h.set(
            "from",
            native("Array.from", |ctx, args| match args.first() {
                Some(value) => super::iterator::collect_iterable(value, ctx.env, ctx.pos.clone()),
                None => Object::Array(Rc::new(RefCell::new(ArrayData::default()))),
            }),
        );
        h.set(
            "of",
            native("Array.of", |_ctx, args| {
                Object::Array(Rc::new(RefCell::new(ArrayData {
                    elements: args.to_vec(),
                })))
            }),
        );
    }
    let call_fn: FnPtr = Rc::new(|_ctx, args| {
        if args.is_empty() {
            return Object::Array(Rc::new(RefCell::new(ArrayData::default())));
        }
        Object::Array(Rc::new(RefCell::new(ArrayData {
            elements: args.to_vec(),
        })))
    });
    hash.borrow_mut().set(
        "__call",
        Object::Builtin(Rc::new(Builtin {
            name: "Array".into(),
            func: call_fn,
            extra: None,
        })),
    );
    Object::Hash(hash)
}

fn string_global() -> Object {
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

fn number_global() -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    {
        let mut h = hash.borrow_mut();
        h.set("MAX_SAFE_INTEGER", Object::Number(9007199254740991.0));
        h.set("MIN_SAFE_INTEGER", Object::Number(-9007199254740991.0));
        h.set("NaN", Object::Number(f64::NAN));
        h.set("POSITIVE_INFINITY", Object::Number(f64::INFINITY));
        h.set("NEGATIVE_INFINITY", Object::Number(f64::NEG_INFINITY));
        h.set("EPSILON", Object::Number(f64::EPSILON));
        h.set(
            "isInteger",
            native("Number.isInteger", |_ctx, args| match args.first() {
                Some(Object::Number(n)) => Object::Boolean(n.fract() == 0.0 && n.is_finite()),
                _ => Object::Boolean(false),
            }),
        );
        h.set(
            "isFinite",
            native("Number.isFinite", |_ctx, args| match args.first() {
                Some(Object::Number(n)) => Object::Boolean(n.is_finite()),
                _ => Object::Boolean(false),
            }),
        );
    }
    let call_fn: FnPtr = Rc::new(|_ctx, args| match args.first() {
        Some(Object::Number(n)) => Object::Number(*n),
        Some(Object::String(s)) => match s.parse::<f64>() {
            Ok(n) => Object::Number(n),
            Err(_) => Object::Number(f64::NAN),
        },
        Some(Object::Boolean(b)) => Object::Number(if *b { 1.0 } else { 0.0 }),
        Some(Object::Null) => Object::Number(0.0),
        _ => Object::Number(0.0),
    });
    hash.borrow_mut().set(
        "__call",
        Object::Builtin(Rc::new(Builtin {
            name: "Number".into(),
            func: call_fn,
            extra: None,
        })),
    );
    Object::Hash(hash)
}

fn boolean_global() -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    let call_fn: FnPtr =
        Rc::new(|_ctx, args| Object::Boolean(args.first().map(|a| a.is_truthy()).unwrap_or(false)));
    hash.borrow_mut().set(
        "__call",
        Object::Builtin(Rc::new(Builtin {
            name: "Boolean".into(),
            func: call_fn,
            extra: None,
        })),
    );
    Object::Hash(hash)
}

// ============================================================================
// Conversion builtins
// ============================================================================

fn builtin_parse_int(_ctx: &mut CallContext, args: &[Object]) -> Object {
    match args.first() {
        Some(Object::Number(n)) => Object::Number(*n as i64 as f64),
        Some(Object::String(s)) => {
            let radix = match args.get(1) {
                Some(Object::Number(r)) => *r as u32,
                _ => 10,
            };
            if !(2..=36).contains(&radix) {
                return new_error(
                    crate::ast::Position::default(),
                    "RangeError: parseInt radix must be between 2 and 36",
                );
            }
            match i64::from_str_radix(s.trim(), radix) {
                Ok(v) => Object::Number(v as f64),
                Err(_) => Object::Number(f64::NAN),
            }
        }
        _ => Object::Number(f64::NAN),
    }
}

fn builtin_parse_float(_ctx: &mut CallContext, args: &[Object]) -> Object {
    match args.first() {
        Some(Object::Number(n)) => Object::Number(*n),
        Some(Object::String(s)) => Object::Number(s.trim().parse::<f64>().unwrap_or(f64::NAN)),
        _ => Object::Number(f64::NAN),
    }
}

fn builtin_is_nan(_ctx: &mut CallContext, args: &[Object]) -> Object {
    Object::Boolean(matches!(args.first(), Some(Object::Number(n)) if n.is_nan()))
}

fn builtin_is_finite(_ctx: &mut CallContext, args: &[Object]) -> Object {
    Object::Boolean(matches!(args.first(), Some(Object::Number(n)) if n.is_finite()))
}

fn chrono_now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ============================================================================
// Promise + timers
// ============================================================================

fn promise_constructor(ctx: &mut CallContext, args: &[Object]) -> Object {
    let promise = Promise::new();
    if let Some(executor) = args.first() {
        let p2 = promise.clone();
        let p3 = promise.clone();
        let resolve_fn: FnPtr = Rc::new(move |_c, a| {
            if let Some(v) = a.first() {
                p2.resolve(v.clone());
            } else {
                p2.resolve(Object::Undefined);
            }
            Object::Undefined
        });
        let reject_fn: FnPtr = Rc::new(move |_c, a| {
            if let Some(v) = a.first() {
                p3.reject(v.clone());
            } else {
                p3.reject(Object::Undefined);
            }
            Object::Undefined
        });
        let resolve = Object::Builtin(Rc::new(Builtin {
            name: "resolve".into(),
            func: resolve_fn,
            extra: None,
        }));
        let reject = Object::Builtin(Rc::new(Builtin {
            name: "reject".into(),
            func: reject_fn,
            extra: None,
        }));
        // Also expose Promise.resolve / Promise.all via static members later.
        let vm = ctx.vm();
        let _ = apply_function(executor, ctx.env, &[resolve, reject], None, ctx.pos.clone());
        let _ = vm;
    }
    Object::Promise(promise)
}

fn register_timers(vm: &Rc<VirtualMachine>) {
    let vm_clone = vm.clone();
    let set_timeout: FnPtr = Rc::new(move |ctx, args| {
        let callback = args.first().cloned().unwrap_or(Object::Undefined);
        let ms = as_num(args.get(1)) as u64;
        let env = ctx.env.clone();
        let id = vm_clone.next_timer_id();
        vm_clone.async_add(1);
        let vm = vm_clone.clone();
        // Single-threaded model: run the callback inline after sleeping so a
        // synchronous top-level script observes it before the process exits.
        if ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        let _ = apply_function(&callback, &env, &[], None, crate::ast::Position::default());
        vm.async_done();
        Object::Number(id as f64)
    });
    vm.set_global(
        "setTimeout",
        Object::Builtin(Rc::new(Builtin {
            name: "setTimeout".into(),
            func: set_timeout,
            extra: None,
        })),
    );

    let sleep_async: FnPtr = Rc::new(|_ctx, args| {
        let ms = as_num(args.first()) as u64;
        let promise = Promise::new();
        if ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        promise.resolve(Object::Undefined);
        Object::Promise(promise)
    });
    vm.set_global(
        "sleepAsync",
        Object::Builtin(Rc::new(Builtin {
            name: "sleepAsync".into(),
            func: sleep_async,
            extra: None,
        })),
    );

    let vm_clone = vm.clone();
    let set_interval: FnPtr = Rc::new(move |ctx, args| {
        let callback = args.first().cloned().unwrap_or(Object::Undefined);
        let ms = as_num(args.get(1)) as u64;
        let env = ctx.env.clone();
        let vm = vm_clone.clone();
        let id = vm.next_timer_id();
        vm.async_add(1);
        // Run a bounded number of times to avoid hanging the process.
        let mut count = 0u32;
        while count < 1000 {
            std::thread::sleep(std::time::Duration::from_millis(ms.max(1)));
            let _ = apply_function(&callback, &env, &[], None, crate::ast::Position::default());
            count += 1;
        }
        vm.async_done();
        Object::Number(id as f64)
    });
    vm.set_global(
        "setInterval",
        Object::Builtin(Rc::new(Builtin {
            name: "setInterval".into(),
            func: set_interval,
            extra: None,
        })),
    );

    attach_promise_statics(vm);
}

fn attach_promise_statics(vm: &Rc<VirtualMachine>) {
    // Re-create Promise as a hash with __call + resolve + all.
    let hash = Rc::new(RefCell::new(HashData::default()));
    hash.borrow_mut().set(
        "__call",
        vm.get_global("Promise").unwrap_or(Object::Undefined),
    );
    let resolve_fn: FnPtr = Rc::new(|_ctx, args| {
        let promise = Promise::new();
        promise.resolve(args.first().cloned().unwrap_or(Object::Undefined));
        Object::Promise(promise)
    });
    hash.borrow_mut().set(
        "resolve",
        Object::Builtin(Rc::new(Builtin {
            name: "Promise.resolve".into(),
            func: resolve_fn,
            extra: None,
        })),
    );
    let reject_fn: FnPtr = Rc::new(|_ctx, args| {
        let promise = Promise::new();
        promise.reject(args.first().cloned().unwrap_or(Object::Undefined));
        Object::Promise(promise)
    });
    hash.borrow_mut().set(
        "reject",
        Object::Builtin(Rc::new(Builtin {
            name: "Promise.reject".into(),
            func: reject_fn,
            extra: None,
        })),
    );
    let all_fn: FnPtr = Rc::new(|_ctx, args| {
        let promise = Promise::new();
        match args.first() {
            Some(Object::Array(arr)) => {
                let items: Vec<Object> = arr.borrow_mut().elements.clone();
                let total = items.len();
                if total == 0 {
                    promise.resolve(Object::Array(Rc::new(RefCell::new(ArrayData::default()))));
                    return Object::Promise(promise);
                }
                let results: Rc<RefCell<Vec<Option<Object>>>> =
                    Rc::new(RefCell::new(vec![None; total]));
                let remaining = Rc::new(std::sync::atomic::AtomicUsize::new(total));

                // Record one settled result at `i`; when the last one lands,
                // collect them all (Undefined for any gap) and resolve. Shared
                // by the promise and non-promise branches below.
                let record_result = |i: usize,
                                     value: Object,
                                     results: &Rc<RefCell<Vec<Option<Object>>>>,
                                     remaining: &Rc<std::sync::atomic::AtomicUsize>,
                                     promise: &Promise| {
                    results.borrow_mut()[i] = Some(value);
                    if remaining.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                        let collected: Vec<Object> = results
                            .borrow_mut()
                            .iter()
                            .map(|o| o.clone().unwrap_or(Object::Undefined))
                            .collect();
                        promise.resolve(Object::Array(Rc::new(RefCell::new(ArrayData {
                            elements: collected,
                        }))));
                    }
                };

                for (i, item) in items.into_iter().enumerate() {
                    match item {
                        Object::Promise(p) => {
                            // Single-threaded: wait inline. If the promise is
                            // already settled this returns immediately.
                            let v = p.wait();
                            if p.state() == PromiseState::Rejected {
                                promise.reject(v);
                                return Object::Promise(promise);
                            }
                            record_result(i, v, &results, &remaining, &promise);
                        }
                        other => {
                            record_result(i, other, &results, &remaining, &promise);
                        }
                    }
                }
            }
            _ => {
                promise.resolve(Object::Array(Rc::new(RefCell::new(ArrayData::default()))));
            }
        }
        Object::Promise(promise)
    });
    hash.borrow_mut().set(
        "all",
        Object::Builtin(Rc::new(Builtin {
            name: "Promise.all".into(),
            func: all_fn,
            extra: None,
        })),
    );

    // Promise.race - resolves/rejects with the first settled promise
    let race_fn: FnPtr = Rc::new(|_ctx, args| {
        let promise = Promise::new();
        match args.first() {
            Some(Object::Array(arr)) => {
                let items: Vec<Object> = arr.borrow_mut().elements.clone();
                if items.is_empty() {
                    // Empty array never settles
                    return Object::Promise(promise);
                }
                let settled = Rc::new(std::sync::atomic::AtomicBool::new(false));
                // Promise.race settles on the first item then breaks; the loop
                // body always exits, which clippy flags as `never_loop`. That is
                // the intended race semantics, so suppress the lint here.
                #[allow(clippy::never_loop)]
                for item in items {
                    match item {
                        Object::Promise(p) => {
                            let v = p.wait();
                            if !settled.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                if p.state() == PromiseState::Rejected {
                                    promise.reject(v);
                                } else {
                                    promise.resolve(v);
                                }
                            }
                            break;
                        }
                        other => {
                            if !settled.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                promise.resolve(other);
                            }
                            break;
                        }
                    }
                }
            }
            _ => {
                // Non-array resolves as undefined
                promise.resolve(Object::Undefined);
            }
        }
        Object::Promise(promise)
    });
    hash.borrow_mut().set(
        "race",
        Object::Builtin(Rc::new(Builtin {
            name: "Promise.race".into(),
            func: race_fn,
            extra: None,
        })),
    );

    // Promise.allSettled - waits for all promises to settle (fulfilled or rejected)
    let all_settled_fn: FnPtr = Rc::new(|_ctx, args| {
        let promise = Promise::new();
        match args.first() {
            Some(Object::Array(arr)) => {
                let items: Vec<Object> = arr.borrow_mut().elements.clone();
                if items.is_empty() {
                    promise.resolve(Object::Array(Rc::new(RefCell::new(ArrayData::default()))));
                    return Object::Promise(promise);
                }

                let mut results = Vec::with_capacity(items.len());
                for item in items {
                    let result_obj = match item {
                        Object::Promise(p) => {
                            let value = p.wait();
                            let result_hash = Rc::new(RefCell::new(HashData::default()));
                            if p.state() == PromiseState::Rejected {
                                result_hash
                                    .borrow_mut()
                                    .set("status".to_string(), str_obj("rejected".to_string()));
                                result_hash.borrow_mut().set("reason".to_string(), value);
                            } else {
                                result_hash
                                    .borrow_mut()
                                    .set("status".to_string(), str_obj("fulfilled".to_string()));
                                result_hash.borrow_mut().set("value".to_string(), value);
                            }
                            Object::Hash(result_hash)
                        }
                        other => {
                            // Non-promise values are treated as fulfilled
                            let result_hash = Rc::new(RefCell::new(HashData::default()));
                            result_hash
                                .borrow_mut()
                                .set("status".to_string(), str_obj("fulfilled".to_string()));
                            result_hash.borrow_mut().set("value".to_string(), other);
                            Object::Hash(result_hash)
                        }
                    };
                    results.push(result_obj);
                }
                promise.resolve(Object::Array(Rc::new(RefCell::new(ArrayData {
                    elements: results,
                }))));
            }
            _ => {
                promise.resolve(Object::Array(Rc::new(RefCell::new(ArrayData::default()))));
            }
        }
        Object::Promise(promise)
    });
    hash.borrow_mut().set(
        "allSettled",
        Object::Builtin(Rc::new(Builtin {
            name: "Promise.allSettled".into(),
            func: all_settled_fn,
            extra: None,
        })),
    );

    vm.set_global("Promise", Object::Hash(hash));

    // Map constructor
    let map_constructor: FnPtr = Rc::new(|_ctx, args| {
        let map_data = Rc::new(RefCell::new(MapData::default()));
        if let Some(Object::Array(arr)) = args.first() {
            for entry in &arr.borrow().elements {
                if let Object::Array(pair) = entry {
                    let pair_data = pair.borrow();
                    if pair_data.elements.len() >= 2 {
                        let key = pair_data.elements[0].clone();
                        let value = pair_data.elements[1].clone();
                        map_data.borrow_mut().set(key, value);
                    }
                }
            }
        }
        Object::Map(map_data)
    });
    vm.set_global(
        "Map",
        Object::Builtin(Rc::new(Builtin {
            name: "Map".into(),
            func: map_constructor,
            extra: None,
        })),
    );

    // Set constructor
    let set_constructor: FnPtr = Rc::new(|_ctx, args| {
        let set_data = Rc::new(RefCell::new(SetData::default()));
        if let Some(Object::Array(arr)) = args.first() {
            for value in &arr.borrow().elements {
                set_data.borrow_mut().add(value.clone());
            }
        }
        Object::Set(set_data)
    });
    vm.set_global(
        "Set",
        Object::Builtin(Rc::new(Builtin {
            name: "Set".into(),
            func: set_constructor,
            extra: None,
        })),
    );

    // Date constructor
    let date_constructor: FnPtr = Rc::new(|_ctx, args| {
        if args.is_empty() {
            // new Date() - current time
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            return Object::Date(now);
        }
        // new Date(milliseconds)
        if let Some(Object::Number(n)) = args.first() {
            return Object::Date(*n as i64);
        }
        // new Date(year, month, day, ...)
        if args.len() >= 2 {
            let year = match args.first() {
                Some(Object::Number(n)) => *n as i32,
                _ => 1970,
            };
            let month = match args.get(1) {
                Some(Object::Number(n)) => (*n as u32 + 1).clamp(1, 12), // JS months are 0-indexed
                _ => 1,
            };
            let day = match args.get(2) {
                Some(Object::Number(n)) => (*n as u32).clamp(1, 31),
                _ => 1,
            };
            let hour = match args.get(3) {
                Some(Object::Number(n)) => (*n as u32).min(23),
                _ => 0,
            };
            let minute = match args.get(4) {
                Some(Object::Number(n)) => (*n as u32).min(59),
                _ => 0,
            };
            let second = match args.get(5) {
                Some(Object::Number(n)) => (*n as u32).min(59),
                _ => 0,
            };
            let millisecond = match args.get(6) {
                Some(Object::Number(n)) => (*n as u32).min(999),
                _ => 0,
            };

            // Convert to milliseconds since epoch
            let ms = crate::stdlib::ms_from_utc_parts(
                year,
                month,
                day,
                hour,
                minute,
                second,
                millisecond,
            );
            return Object::Date(ms);
        }
        Object::Date(0)
    });
    vm.set_global(
        "Date",
        Object::Builtin(Rc::new(Builtin {
            name: "Date".into(),
            func: date_constructor,
            extra: None,
        })),
    );
}

// ============================================================================
// Method tables
// ============================================================================

pub fn array_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "push" => Some(arr_push),
        "pop" => Some(arr_pop),
        "shift" => Some(arr_shift),
        "unshift" => Some(arr_unshift),
        "map" => Some(arr_map),
        "filter" => Some(arr_filter),
        "forEach" => Some(arr_for_each),
        "reduce" => Some(arr_reduce),
        "reduceRight" => Some(arr_reduce_right),
        "find" => Some(arr_find),
        "findIndex" => Some(arr_find_index),
        "some" => Some(arr_some),
        "every" => Some(arr_every),
        "includes" => Some(arr_includes),
        "indexOf" => Some(arr_index_of),
        "join" => Some(arr_join),
        "slice" => Some(arr_slice),
        "splice" => Some(arr_splice),
        "concat" => Some(arr_concat),
        "reverse" => Some(arr_reverse),
        "sort" => Some(arr_sort),
        "flat" => Some(arr_flat),
        "flatMap" => Some(arr_flat_map),
        "fill" => Some(arr_fill),
        "copyWithin" => Some(arr_copy_within),
        "keys" => Some(arr_keys),
        "entries" => Some(arr_entries),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}

fn receiver_array(ctx: &CallContext) -> Option<Rc<RefCell<ArrayData>>> {
    match &ctx.receiver {
        Some(Object::Array(a)) => Some(a.clone()),
        _ => None,
    }
}

// Methods read their receiver from `CallContext::receiver`, which apply_function
// populates from the bound Builtin's `extra` field. No thread-local state.

fn active_array(ctx: &CallContext) -> Option<Rc<RefCell<ArrayData>>> {
    receiver_array(ctx)
}
fn active_string(ctx: &CallContext) -> Option<Rc<String>> {
    match &ctx.receiver {
        Some(Object::String(s)) => Some(s.clone()),
        _ => None,
    }
}
fn active_number(ctx: &CallContext) -> Option<f64> {
    match &ctx.receiver {
        Some(Object::Number(n)) => Some(*n),
        _ => None,
    }
}
fn active_promise(ctx: &CallContext) -> Option<Rc<Promise>> {
    match &ctx.receiver {
        Some(Object::Promise(p)) => Some(p.clone()),
        _ => None,
    }
}
fn active_regexp(ctx: &CallContext) -> Option<Rc<RegexpData>> {
    match &ctx.receiver {
        Some(Object::Regexp(r)) => Some(r.clone()),
        _ => None,
    }
}

fn arr_push(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        a.borrow_mut().elements.extend_from_slice(args);
        return Object::Number(a.borrow_mut().elements.len() as f64);
    }
    Object::Undefined
}
fn arr_pop(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        return a.borrow_mut().elements.pop().unwrap_or(Object::Undefined);
    }
    Object::Undefined
}
fn arr_shift(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        let mut arr = a.borrow_mut();
        if arr.elements.is_empty() {
            return Object::Undefined;
        }
        return arr.elements.remove(0);
    }
    Object::Undefined
}
fn arr_unshift(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        let mut arr = a.borrow_mut();
        let mut new_elems = args.to_vec();
        new_elems.append(&mut arr.elements);
        arr.elements = new_elems;
        return Object::Number(arr.elements.len() as f64);
    }
    Object::Undefined
}
fn arr_map(ctx: &mut CallContext, args: &[Object]) -> Object {
    let a = match active_array(ctx) {
        Some(a) => a,
        None => return Object::Undefined,
    };
    let cb = match args.first() {
        Some(o) => o.clone(),
        None => return Object::Undefined,
    };
    let elems = a.borrow_mut().elements.clone();
    let mut out = Vec::with_capacity(elems.len());
    for (i, e) in elems.into_iter().enumerate() {
        let r = apply_function(
            &cb,
            ctx.env,
            &[e, Object::Number(i as f64)],
            None,
            ctx.pos.clone(),
        );
        out.push(r);
    }
    Object::Array(Rc::new(RefCell::new(ArrayData { elements: out })))
}
fn arr_filter(ctx: &mut CallContext, args: &[Object]) -> Object {
    let a = match active_array(ctx) {
        Some(a) => a,
        None => return Object::Undefined,
    };
    let cb = match args.first() {
        Some(o) => o.clone(),
        None => return Object::Undefined,
    };
    let elems = a.borrow_mut().elements.clone();
    let mut out = Vec::new();
    for (i, e) in elems.into_iter().enumerate() {
        let r = apply_function(
            &cb,
            ctx.env,
            &[e.clone(), Object::Number(i as f64)],
            None,
            ctx.pos.clone(),
        );
        if r.is_truthy() {
            out.push(e);
        }
    }
    Object::Array(Rc::new(RefCell::new(ArrayData { elements: out })))
}
fn arr_for_each(ctx: &mut CallContext, args: &[Object]) -> Object {
    let a = match active_array(ctx) {
        Some(a) => a,
        None => return Object::Undefined,
    };
    let cb = match args.first() {
        Some(o) => o.clone(),
        None => return Object::Undefined,
    };
    let elems = a.borrow_mut().elements.clone();
    for (i, e) in elems.into_iter().enumerate() {
        apply_function(
            &cb,
            ctx.env,
            &[e, Object::Number(i as f64)],
            None,
            ctx.pos.clone(),
        );
    }
    Object::Undefined
}
fn arr_reduce(ctx: &mut CallContext, args: &[Object]) -> Object {
    let a = match active_array(ctx) {
        Some(a) => a,
        None => return Object::Undefined,
    };
    let cb = match args.first() {
        Some(o) => o.clone(),
        None => return Object::Undefined,
    };
    let elems = a.borrow_mut().elements.clone();
    let (mut acc, start) = if args.len() >= 2 {
        (args[1].clone(), 0)
    } else if elems.is_empty() {
        return new_error(
            ctx.pos.clone(),
            "TypeError: Reduce of empty array with no initial value",
        );
    } else {
        (elems[0].clone(), 1)
    };
    for (i, e) in elems.into_iter().enumerate().skip(start) {
        acc = apply_function(
            &cb,
            ctx.env,
            &[acc, e, Object::Number(i as f64)],
            None,
            ctx.pos.clone(),
        );
    }
    acc
}

fn arr_reduce_right(ctx: &mut CallContext, args: &[Object]) -> Object {
    let a = match active_array(ctx) {
        Some(a) => a,
        None => return Object::Undefined,
    };
    let cb = match args.first() {
        Some(o) => o.clone(),
        None => return Object::Undefined,
    };
    let elems = a.borrow_mut().elements.clone();
    let len = elems.len();

    let (mut acc, start) = if args.len() >= 2 {
        (args[1].clone(), len)
    } else if elems.is_empty() {
        return new_error(
            ctx.pos.clone(),
            "TypeError: Reduce of empty array with no initial value",
        );
    } else {
        (elems[len - 1].clone(), len - 1)
    };

    for i in (0..start).rev() {
        acc = apply_function(
            &cb,
            ctx.env,
            &[acc, elems[i].clone(), Object::Number(i as f64)],
            None,
            ctx.pos.clone(),
        );
    }
    acc
}

fn arr_find(ctx: &mut CallContext, args: &[Object]) -> Object {
    let a = match active_array(ctx) {
        Some(a) => a,
        None => return Object::Undefined,
    };
    let cb = match args.first() {
        Some(o) => o.clone(),
        None => return Object::Undefined,
    };
    let elems = a.borrow_mut().elements.clone();
    for (i, e) in elems.into_iter().enumerate() {
        let r = apply_function(
            &cb,
            ctx.env,
            &[e.clone(), Object::Number(i as f64)],
            None,
            ctx.pos.clone(),
        );
        if r.is_truthy() {
            return e;
        }
    }
    Object::Undefined
}
fn arr_find_index(ctx: &mut CallContext, args: &[Object]) -> Object {
    let a = match active_array(ctx) {
        Some(a) => a,
        None => return Object::Undefined,
    };
    let cb = match args.first() {
        Some(o) => o.clone(),
        None => return Object::Undefined,
    };
    let elems = a.borrow_mut().elements.clone();
    for (i, e) in elems.into_iter().enumerate() {
        let r = apply_function(
            &cb,
            ctx.env,
            &[e, Object::Number(i as f64)],
            None,
            ctx.pos.clone(),
        );
        if r.is_truthy() {
            return Object::Number(i as f64);
        }
    }
    Object::Number(-1.0)
}
fn arr_some(ctx: &mut CallContext, args: &[Object]) -> Object {
    let a = match active_array(ctx) {
        Some(a) => a,
        None => return Object::Undefined,
    };
    let cb = match args.first() {
        Some(o) => o.clone(),
        None => return Object::Undefined,
    };
    let elems = a.borrow_mut().elements.clone();
    for (i, e) in elems.into_iter().enumerate() {
        let r = apply_function(
            &cb,
            ctx.env,
            &[e, Object::Number(i as f64)],
            None,
            ctx.pos.clone(),
        );
        if r.is_truthy() {
            return Object::Boolean(true);
        }
    }
    Object::Boolean(false)
}
fn arr_every(ctx: &mut CallContext, args: &[Object]) -> Object {
    let a = match active_array(ctx) {
        Some(a) => a,
        None => return Object::Undefined,
    };
    let cb = match args.first() {
        Some(o) => o.clone(),
        None => return Object::Undefined,
    };
    let elems = a.borrow_mut().elements.clone();
    for (i, e) in elems.into_iter().enumerate() {
        let r = apply_function(
            &cb,
            ctx.env,
            &[e, Object::Number(i as f64)],
            None,
            ctx.pos.clone(),
        );
        if !r.is_truthy() {
            return Object::Boolean(false);
        }
    }
    Object::Boolean(true)
}
fn arr_includes(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        let target = args.first();
        for e in a.borrow_mut().elements.iter() {
            if let Some(t) = target {
                if strict_equal(e, t) {
                    return Object::Boolean(true);
                }
            }
        }
    }
    Object::Boolean(false)
}
fn arr_index_of(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        let target = args.first();
        for (i, e) in a.borrow_mut().elements.iter().enumerate() {
            if let Some(t) = target {
                if strict_equal(e, t) {
                    return Object::Number(i as f64);
                }
            }
        }
    }
    Object::Number(-1.0)
}
fn arr_join(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        let sep = match args.first() {
            Some(Object::String(s)) => s.to_string(),
            _ => ",".into(),
        };
        let parts: Vec<String> = a
            .borrow_mut()
            .elements
            .iter()
            .map(|e| e.inspect())
            .collect();
        return str_obj(parts.join(&sep));
    }
    str_obj("")
}
fn arr_slice(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        let len = a.borrow_mut().elements.len() as isize;
        let start = normalize_index(as_num(args.first()) as isize, len);
        let end = match args.get(1) {
            Some(Object::Number(n)) => normalize_index(*n as isize, len),
            _ => len,
        };
        let s = start.max(0) as usize;
        let e = end.max(0).min(len) as usize;
        let slice = if s < e {
            a.borrow_mut().elements[s..e].to_vec()
        } else {
            Vec::new()
        };
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: slice })));
    }
    Object::Array(Rc::new(RefCell::new(ArrayData::default())))
}
fn arr_splice(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(a) = active_array(ctx) else {
        return Object::Array(Rc::new(RefCell::new(ArrayData::default())));
    };
    let mut arr = a.borrow_mut();
    let len = arr.elements.len() as isize;
    let start = normalize_index(as_num(args.first()) as isize, len)
        .max(0)
        .min(len) as usize;
    let delete_count = if args.is_empty() {
        0
    } else {
        match args.get(1) {
            Some(Object::Number(n)) => (*n as isize).max(0).min(len - start as isize) as usize,
            Some(_) => 0,
            None => (len as usize).saturating_sub(start),
        }
    };
    let items: Vec<Object> = args.iter().skip(2).cloned().collect();
    let removed: Vec<Object> = arr
        .elements
        .splice(start..start + delete_count, items)
        .collect();
    Object::Array(Rc::new(RefCell::new(ArrayData { elements: removed })))
}
fn arr_concat(ctx: &mut CallContext, args: &[Object]) -> Object {
    let mut out = match active_array(ctx) {
        Some(a) => a.borrow_mut().elements.clone(),
        None => Vec::new(),
    };
    for a in args {
        match a {
            Object::Array(arr) => out.extend(arr.borrow_mut().elements.iter().cloned()),
            other => out.push(other.clone()),
        }
    }
    Object::Array(Rc::new(RefCell::new(ArrayData { elements: out })))
}
fn arr_reverse(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        a.borrow_mut().elements.reverse();
        return Object::Array(a);
    }
    Object::Undefined
}
fn arr_sort(ctx: &mut CallContext, args: &[Object]) -> Object {
    let a = match active_array(ctx) {
        Some(a) => a,
        None => return Object::Undefined,
    };
    let cb = match args.first() {
        Some(o) => o.clone(),
        None => {
            return new_error(
                ctx.pos.clone(),
                "TypeError: sort requires a compare function",
            )
        }
    };
    // Simple insertion sort invoking the comparator (stable).
    let mut elems = a.borrow_mut().elements.clone();
    let n = elems.len();
    for i in 1..n {
        let mut j = i;
        while j > 0 {
            let cmp = apply_function(
                &cb,
                ctx.env,
                &[elems[j - 1].clone(), elems[j].clone()],
                None,
                ctx.pos.clone(),
            );
            let less = match &cmp {
                Object::Number(n) => *n > 0.0,
                _ => false,
            };
            if less {
                elems.swap(j - 1, j);
                j -= 1;
            } else {
                break;
            }
        }
    }
    a.borrow_mut().elements = elems;
    Object::Array(a)
}
fn arr_flat(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        let mut out = Vec::new();
        for e in a.borrow_mut().elements.iter() {
            match e {
                Object::Array(inner) => out.extend(inner.borrow_mut().elements.iter().cloned()),
                other => out.push(other.clone()),
            }
        }
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: out })));
    }
    Object::Undefined
}
fn arr_flat_map(ctx: &mut CallContext, args: &[Object]) -> Object {
    let a = match active_array(ctx) {
        Some(a) => a,
        None => return Object::Undefined,
    };
    let cb = match args.first() {
        Some(o) => o.clone(),
        None => return Object::Undefined,
    };
    let elems = a.borrow_mut().elements.clone();
    let mut out = Vec::new();
    for (i, e) in elems.into_iter().enumerate() {
        let r = apply_function(
            &cb,
            ctx.env,
            &[e, Object::Number(i as f64)],
            None,
            ctx.pos.clone(),
        );
        match r {
            Object::Array(inner) => out.extend(inner.borrow_mut().elements.iter().cloned()),
            other => out.push(other),
        }
    }
    Object::Array(Rc::new(RefCell::new(ArrayData { elements: out })))
}
fn arr_fill(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        let val = args.first().cloned().unwrap_or(Object::Undefined);
        let len = a.borrow_mut().elements.len();
        let start = as_num(args.get(1)) as usize;
        let end = match args.get(2) {
            Some(Object::Number(n)) => *n as usize,
            _ => len,
        };
        let s = start.min(len);
        let e = end.min(len);
        let mut arr = a.borrow_mut();
        for i in s..e {
            arr.elements[i] = val.clone();
        }
        drop(arr);
        return Object::Array(a);
    }
    Object::Undefined
}

fn arr_copy_within(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        let mut arr = a.borrow_mut();
        let length = arr.elements.len() as isize;
        if length == 0 || args.len() < 2 {
            drop(arr);
            return Object::Array(a);
        }

        let target = normalize_index(as_num(args.first()) as isize, length) as usize;
        let start = normalize_index(as_num(args.get(1)) as isize, length) as usize;
        let end = if args.len() > 2 {
            normalize_index(as_num(args.get(2)) as isize, length).min(length) as usize
        } else {
            length as usize
        };

        let copy_count = end.saturating_sub(start);
        if copy_count == 0 || target >= length as usize {
            drop(arr);
            return Object::Array(a);
        }

        // Clone the range to copy
        let to_copy: Vec<Object> = arr.elements[start..end].to_vec();
        // Copy into target position
        for (i, item) in to_copy.iter().enumerate() {
            if target + i < arr.elements.len() {
                arr.elements[target + i] = item.clone();
            }
        }
        drop(arr);
        return Object::Array(a);
    }
    Object::Undefined
}

fn arr_keys(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        let len = a.borrow().elements.len();
        let keys: Vec<Object> = (0..len).map(|i| Object::Number(i as f64)).collect();
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: keys })));
    }
    Object::Array(Rc::new(RefCell::new(ArrayData::default())))
}

fn arr_entries(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(a) = active_array(ctx) {
        let arr = a.borrow();
        let entries: Vec<Object> = arr
            .elements
            .iter()
            .enumerate()
            .map(|(i, elem)| {
                Object::Array(Rc::new(RefCell::new(ArrayData {
                    elements: vec![Object::Number(i as f64), elem.clone()],
                })))
            })
            .collect();
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: entries })));
    }
    Object::Array(Rc::new(RefCell::new(ArrayData::default())))
}

fn normalize_index(idx: isize, len: isize) -> isize {
    if idx < 0 {
        (len + idx).max(0)
    } else {
        idx
    }
}

// ============================================================================
// String methods
// ============================================================================

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
                return Object::Number(m.start() as f64);
            }
            Object::Number(-1.0)
        }
        _ => {
            // Convert argument to string pattern
            let pattern = args[0].inspect();
            if let Ok(re) = regex::Regex::new(&regex::escape(&pattern)) {
                if let Some(m) = re.find(&s) {
                    return Object::Number(m.start() as f64);
                }
            }
            Object::Number(-1.0)
        }
    }
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

// ============================================================================
// Number / Promise / RegExp methods
// ============================================================================

pub fn number_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "toFixed" => Some(num_to_fixed),
        "toExponential" => Some(num_to_exponential),
        "toString" => Some(num_to_string),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}
fn num_to_fixed(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(n) = active_number(ctx) {
        let digits = as_num(args.first()) as usize;
        return str_obj(format!("{:.*}", digits, n));
    }
    str_obj("0")
}
fn num_to_exponential(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(n) = active_number(ctx) {
        let digits = if args.is_empty() {
            // Default precision
            format!("{:e}", n)
        } else {
            let precision = as_num(args.first()) as usize;
            format!("{:.prec$e}", n, prec = precision)
        };
        return str_obj(digits);
    }
    str_obj("0")
}
fn num_to_string(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(n) = active_number(ctx) {
        if let Some(Object::Number(radix)) = args.first() {
            let r = *radix as u32;
            if r == 16 {
                return str_obj(format!("{:x}", n as i64));
            }
            if r == 2 {
                return str_obj(format!("{:b}", n as i64));
            }
        }
        return str_obj(format_number(n));
    }
    str_obj("0")
}

pub fn promise_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "then" => Some(prom_then),
        "catch" => Some(prom_catch),
        "finally" => Some(prom_finally),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}

pub fn map_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "set" => Some(map_set),
        "get" => Some(map_get),
        "has" => Some(map_has),
        "delete" => Some(map_delete),
        "clear" => Some(map_clear),
        "keys" => Some(map_keys),
        "values" => Some(map_values),
        "entries" => Some(map_entries),
        "forEach" => Some(map_for_each),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}

pub fn set_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "add" => Some(set_add),
        "has" => Some(set_has),
        "delete" => Some(set_delete),
        "clear" => Some(set_clear),
        "values" => Some(set_values),
        "entries" => Some(set_entries),
        "forEach" => Some(set_for_each),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}

pub fn date_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "getTime" | "valueOf" => Some(date_get_time),
        "getFullYear" => Some(date_get_full_year),
        "getMonth" => Some(date_get_month),
        "getDate" => Some(date_get_date),
        "getDay" => Some(date_get_day),
        "getHours" => Some(date_get_hours),
        "getMinutes" => Some(date_get_minutes),
        "getSeconds" => Some(date_get_seconds),
        "getMilliseconds" => Some(date_get_milliseconds),
        "toISOString" => Some(date_to_iso_string),
        "toDateString" => Some(date_to_date_string),
        "toTimeString" => Some(date_to_time_string),
        "toString" => Some(date_to_string),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}
fn prom_then(ctx: &mut CallContext, args: &[Object]) -> Object {
    let p = match active_promise(ctx) {
        Some(p) => p,
        None => return Object::Undefined,
    };
    let on_fulfilled = args.first().cloned();
    let next = Promise::new();
    let env = ctx.env.clone();
    let pos = ctx.pos.clone();
    let next_for_continuation = next.clone();
    p.add_continuation(Box::new(move |state, result| {
        if state == PromiseState::Rejected {
            next_for_continuation.reject(result);
            return;
        }
        match &on_fulfilled {
            Some(f) => {
                let r = apply_function(f, &env, &[result], None, pos.clone());
                resolve_chained_promise(&next_for_continuation, r);
            }
            None => next_for_continuation.resolve(result),
        }
    }));
    Object::Promise(next)
}

fn prom_catch(ctx: &mut CallContext, args: &[Object]) -> Object {
    let p = match active_promise(ctx) {
        Some(p) => p,
        None => return Object::Undefined,
    };
    let on_reject = args.first().cloned();
    let next = Promise::new();
    let env = ctx.env.clone();
    let pos = ctx.pos.clone();
    let next_for_continuation = next.clone();
    p.add_continuation(Box::new(move |state, result| {
        if state == PromiseState::Fulfilled {
            next_for_continuation.resolve(result);
            return;
        }
        match &on_reject {
            Some(f) => {
                let r = apply_function(f, &env, &[result], None, pos.clone());
                resolve_chained_promise(&next_for_continuation, r);
            }
            None => next_for_continuation.reject(result),
        }
    }));
    Object::Promise(next)
}

fn prom_finally(ctx: &mut CallContext, args: &[Object]) -> Object {
    let p = match active_promise(ctx) {
        Some(p) => p,
        None => return Object::Undefined,
    };
    let on_finally = args.first().cloned();
    let next = Promise::new();
    let env = ctx.env.clone();
    let pos = ctx.pos.clone();
    let next_for_continuation = next.clone();
    p.add_continuation(Box::new(move |state, result| {
        if let Some(f) = &on_finally {
            let r = apply_function(f, &env, &[], None, pos.clone());
            if r.is_runtime_error() {
                next_for_continuation.reject(r);
                return;
            }
        }
        if state == PromiseState::Rejected {
            next_for_continuation.reject(result);
        } else {
            next_for_continuation.resolve(result);
        }
    }));
    Object::Promise(next)
}

fn resolve_chained_promise(next: &Rc<Promise>, result: Object) {
    match result {
        Object::Promise(promise) => {
            let next = next.clone();
            promise.add_continuation(Box::new(move |state, value| {
                if state == PromiseState::Rejected {
                    next.reject(value);
                } else {
                    next.resolve(value);
                }
            }));
        }
        other if other.is_runtime_error() => next.reject(other),
        other => next.resolve(other),
    }
}

// Map methods
fn active_map(ctx: &CallContext) -> Option<Rc<RefCell<MapData>>> {
    ctx.receiver.as_ref().and_then(|t| {
        if let Object::Map(m) = t {
            Some(Rc::clone(m))
        } else {
            None
        }
    })
}

fn map_set(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        if let Some(key) = args.first() {
            let value = args.get(1).cloned().unwrap_or(Object::Undefined);
            map.borrow_mut().set(key.clone(), value);
            return ctx.receiver.clone().unwrap_or(Object::Undefined);
        }
    }
    Object::Undefined
}

fn map_get(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        if let Some(key) = args.first() {
            return map.borrow().get(key).cloned().unwrap_or(Object::Undefined);
        }
    }
    Object::Undefined
}

fn map_has(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        if let Some(key) = args.first() {
            return Object::Boolean(map.borrow().has(key));
        }
    }
    Object::Boolean(false)
}

fn map_delete(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        if let Some(key) = args.first() {
            return Object::Boolean(map.borrow_mut().delete(key));
        }
    }
    Object::Boolean(false)
}

fn map_clear(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        map.borrow_mut().clear();
    }
    Object::Undefined
}

fn map_keys(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        let keys: Vec<Object> = map
            .borrow()
            .entries
            .iter()
            .map(|(_, k, _)| k.clone())
            .collect();
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: keys })));
    }
    Object::Array(Rc::new(RefCell::new(ArrayData::default())))
}

fn map_values(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        let values: Vec<Object> = map
            .borrow()
            .entries
            .iter()
            .map(|(_, _, v)| v.clone())
            .collect();
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: values })));
    }
    Object::Array(Rc::new(RefCell::new(ArrayData::default())))
}

fn map_entries(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        let entries: Vec<Object> = map
            .borrow()
            .entries
            .iter()
            .map(|(_, k, v)| {
                Object::Array(Rc::new(RefCell::new(ArrayData {
                    elements: vec![k.clone(), v.clone()],
                })))
            })
            .collect();
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: entries })));
    }
    Object::Array(Rc::new(RefCell::new(ArrayData::default())))
}

fn map_for_each(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        if let Some(callback) = args.first() {
            let entries = map.borrow().entries.clone();
            for (_, k, v) in entries {
                let _ = apply_function(callback, ctx.env, &[v, k], None, ctx.pos.clone());
            }
        }
    }
    Object::Undefined
}

// Set methods
fn active_set(ctx: &CallContext) -> Option<Rc<RefCell<SetData>>> {
    ctx.receiver.as_ref().and_then(|t| {
        if let Object::Set(s) = t {
            Some(Rc::clone(s))
        } else {
            None
        }
    })
}

fn set_add(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        if let Some(value) = args.first() {
            set.borrow_mut().add(value.clone());
            return ctx.receiver.clone().unwrap_or(Object::Undefined);
        }
    }
    Object::Undefined
}

fn set_has(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        if let Some(value) = args.first() {
            return Object::Boolean(set.borrow().has(value));
        }
    }
    Object::Boolean(false)
}

fn set_delete(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        if let Some(value) = args.first() {
            return Object::Boolean(set.borrow_mut().delete(value));
        }
    }
    Object::Boolean(false)
}

fn set_clear(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        set.borrow_mut().clear();
    }
    Object::Undefined
}

fn set_values(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        let values: Vec<Object> = set
            .borrow()
            .entries
            .iter()
            .map(|(_, v)| v.clone())
            .collect();
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: values })));
    }
    Object::Array(Rc::new(RefCell::new(ArrayData::default())))
}

fn set_entries(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        let entries: Vec<Object> = set
            .borrow()
            .entries
            .iter()
            .map(|(_, v)| {
                Object::Array(Rc::new(RefCell::new(ArrayData {
                    elements: vec![v.clone(), v.clone()],
                })))
            })
            .collect();
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: entries })));
    }
    Object::Array(Rc::new(RefCell::new(ArrayData::default())))
}

fn set_for_each(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        if let Some(callback) = args.first() {
            let entries = set.borrow().entries.clone();
            for (_, v) in entries {
                let _ = apply_function(callback, ctx.env, &[v.clone(), v], None, ctx.pos.clone());
            }
        }
    }
    Object::Undefined
}

// Date methods
fn active_date(ctx: &CallContext) -> Option<i64> {
    ctx.receiver.as_ref().and_then(|t| {
        if let Object::Date(ms) = t {
            Some(*ms)
        } else {
            None
        }
    })
}

// Helper function to get UTC date parts from milliseconds
fn utc_parts(ms: i64) -> (i32, u32, u32, u32, u32, u32, u32) {
    // Using the stdlib function
    crate::stdlib::utc_parts_from_ms(ms)
}

fn date_get_time(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        return Object::Number(ms as f64);
    }
    Object::Undefined
}

fn date_get_full_year(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (year, _, _, _, _, _, _) = utc_parts(ms);
        return Object::Number(year as f64);
    }
    Object::Undefined
}

fn date_get_month(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, month, _, _, _, _, _) = utc_parts(ms);
        // JavaScript months are 0-indexed
        return Object::Number((month - 1) as f64);
    }
    Object::Undefined
}

fn date_get_date(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, _, day, _, _, _, _) = utc_parts(ms);
        return Object::Number(day as f64);
    }
    Object::Undefined
}

fn date_get_day(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        // Day of week: 0 = Sunday, 6 = Saturday
        // Unix epoch (1970-01-01) was a Thursday (4)
        let days = ms / 86400000;
        let day_of_week = ((days + 4) % 7 + 7) % 7;
        return Object::Number(day_of_week as f64);
    }
    Object::Undefined
}

fn date_get_hours(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, _, _, hour, _, _, _) = utc_parts(ms);
        return Object::Number(hour as f64);
    }
    Object::Undefined
}

fn date_get_minutes(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, _, _, _, minute, _, _) = utc_parts(ms);
        return Object::Number(minute as f64);
    }
    Object::Undefined
}

fn date_get_seconds(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, _, _, _, _, second, _) = utc_parts(ms);
        return Object::Number(second as f64);
    }
    Object::Undefined
}

fn date_get_milliseconds(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, _, _, _, _, _, millisecond) = utc_parts(ms);
        return Object::Number(millisecond as f64);
    }
    Object::Undefined
}

fn date_to_iso_string(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        return str_obj(crate::stdlib::format_epoch_ms_utc(ms));
    }
    str_obj("")
}

fn date_to_date_string(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (year, month, day, _, _, _, _) = utc_parts(ms);
        let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let month_names = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let days = ms / 86400000;
        let day_of_week = ((days + 4) % 7 + 7) % 7;
        return str_obj(format!(
            "{} {} {:02} {}",
            day_names[day_of_week as usize],
            month_names[(month - 1) as usize],
            day,
            year
        ));
    }
    str_obj("")
}

fn date_to_time_string(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        let (_, _, _, hour, minute, second, _) = utc_parts(ms);
        return str_obj(format!("{:02}:{:02}:{:02} GMT", hour, minute, second));
    }
    str_obj("")
}

fn date_to_string(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(ms) = active_date(ctx) {
        return str_obj(crate::stdlib::format_epoch_ms_utc(ms));
    }
    str_obj("")
}

pub fn regexp_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "test" => Some(rex_test),
        "exec" => Some(rex_exec),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}
fn rex_test(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(r) = active_regexp(ctx) {
        if let Some(Object::String(s)) = args.first() {
            return Object::Boolean(r.re.is_match(s));
        }
    }
    Object::Boolean(false)
}
fn rex_exec(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(r) = active_regexp(ctx) {
        if let Some(Object::String(s)) = args.first() {
            if let Some(m) = r.re.find(s) {
                let elems = vec![str_obj(m.as_str().to_string())];
                return Object::Array(Rc::new(RefCell::new(ArrayData { elements: elems })));
            }
            return Object::Null;
        }
    }
    Object::Null
}
