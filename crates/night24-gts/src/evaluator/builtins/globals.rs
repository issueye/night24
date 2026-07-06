use crate::object::*;

pub(super) fn builtin_parse_int(_ctx: &mut CallContext, args: &[Object]) -> Object {
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

pub(super) fn builtin_parse_float(_ctx: &mut CallContext, args: &[Object]) -> Object {
    match args.first() {
        Some(Object::Number(n)) => Object::Number(*n),
        Some(Object::String(s)) => Object::Number(s.trim().parse::<f64>().unwrap_or(f64::NAN)),
        _ => Object::Number(f64::NAN),
    }
}

pub(super) fn builtin_is_nan(_ctx: &mut CallContext, args: &[Object]) -> Object {
    Object::Boolean(matches!(args.first(), Some(Object::Number(n)) if n.is_nan()))
}

pub(super) fn builtin_is_finite(_ctx: &mut CallContext, args: &[Object]) -> Object {
    Object::Boolean(matches!(args.first(), Some(Object::Number(n)) if n.is_finite()))
}
