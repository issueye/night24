use std::cell::RefCell;
use std::rc::Rc;

use crate::object::*;

use super::{as_num, native};

pub(super) fn math_object() -> Object {
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
