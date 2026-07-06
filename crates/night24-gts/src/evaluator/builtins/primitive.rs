use std::cell::RefCell;
use std::rc::Rc;

use crate::object::*;

use super::{as_num, native, FnPtr};

pub(super) fn number_global() -> Object {
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

pub(super) fn boolean_global() -> Object {
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

pub fn number_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "toFixed" => Some(num_to_fixed),
        "toExponential" => Some(num_to_exponential),
        "toString" => Some(num_to_string),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}

fn active_number(ctx: &CallContext) -> Option<f64> {
    match &ctx.receiver {
        Some(Object::Number(n)) => Some(*n),
        _ => None,
    }
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
