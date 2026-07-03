use super::super::helpers::*;
use crate::object::{new_error, num_obj, strict_equal, CallContext, Object};

pub(crate) fn collections_module() -> Object {
    module(vec![
        ("unique", native("collections.unique", collections_unique)),
        ("chunk", native("collections.chunk", collections_chunk)),
        (
            "flatten",
            native("collections.flatten", collections_flatten),
        ),
        ("sample", native("collections.sample", collections_sample)),
        (
            "shuffle",
            native("collections.shuffle", collections_shuffle),
        ),
        ("range", native("collections.range", collections_range)),
    ])
}

pub(crate) fn collections_unique(ctx: &mut CallContext, args: &[Object]) -> Object {
    let arr = match args.first() {
        Some(Object::Array(a)) => a.clone(),
        Some(_) => return new_error(ctx.pos.clone(), "collections.unique expects array"),
        None => return new_error(ctx.pos.clone(), "collections.unique requires array"),
    };
    let mut seen: Vec<Object> = Vec::new();
    let mut out: Vec<Object> = Vec::new();
    for elem in arr.borrow().elements.iter() {
        let mut found = false;
        for prev in &seen {
            if strict_equal(elem, prev) {
                found = true;
                break;
            }
        }
        if !found {
            seen.push(elem.clone());
            out.push(elem.clone());
        }
    }
    array(out)
}

pub(crate) fn collections_chunk(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.len() < 2 {
        return new_error(ctx.pos.clone(), "collections.chunk requires array and size");
    }
    let arr = match &args[0] {
        Object::Array(a) => a.clone(),
        _ => return new_error(ctx.pos.clone(), "collections.chunk expects array"),
    };
    let size = match &args[1] {
        Object::Number(n) => *n,
        _ => return new_error(ctx.pos.clone(), "collections.chunk expects number size"),
    };
    if size <= 0.0 {
        return new_error(ctx.pos.clone(), "collections.chunk size must be positive");
    }
    let size = size as usize;
    let elements = arr.borrow().elements.clone();
    let chunks: Vec<Object> = elements.chunks(size).map(|c| array(c.to_vec())).collect();
    array(chunks)
}

pub(crate) fn collections_flatten(ctx: &mut CallContext, args: &[Object]) -> Object {
    let arr = match args.first() {
        Some(Object::Array(a)) => a.clone(),
        Some(_) => return new_error(ctx.pos.clone(), "collections.flatten expects array"),
        None => return new_error(ctx.pos.clone(), "collections.flatten requires array"),
    };
    let mut out = Vec::new();
    for elem in arr.borrow().elements.iter() {
        match elem {
            Object::Array(inner) => out.extend(inner.borrow().elements.iter().cloned()),
            other => out.push(other.clone()),
        }
    }
    array(out)
}

pub(crate) fn collections_sample(ctx: &mut CallContext, args: &[Object]) -> Object {
    let arr = match args.first() {
        Some(Object::Array(a)) => a.clone(),
        Some(_) => return new_error(ctx.pos.clone(), "collections.sample expects array"),
        None => return new_error(ctx.pos.clone(), "collections.sample requires array"),
    };
    let elements = arr.borrow();
    if elements.elements.is_empty() {
        return Object::Undefined;
    }
    let len = elements.elements.len();
    match bounded_random_u64(ctx, "collections.sample", len as u64) {
        Ok(idx) => elements.elements[idx as usize].clone(),
        Err(err) => err,
    }
}

pub(crate) fn collections_shuffle(ctx: &mut CallContext, args: &[Object]) -> Object {
    let arr = match args.first() {
        Some(Object::Array(a)) => a.clone(),
        Some(_) => return new_error(ctx.pos.clone(), "collections.shuffle expects array"),
        None => return new_error(ctx.pos.clone(), "collections.shuffle requires array"),
    };
    let mut elements = arr.borrow().elements.clone();
    let len = elements.len();
    for i in (1..len).rev() {
        match bounded_random_u64(ctx, "collections.shuffle", (i + 1) as u64) {
            Ok(j) => elements.swap(i, j as usize),
            Err(err) => return err,
        }
    }
    array(elements)
}

pub(crate) fn collections_range(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.is_empty() {
        return new_error(
            ctx.pos.clone(),
            "collections.range requires at least end value",
        );
    }
    let (start, end, step) = match args.len() {
        1 => {
            let end = match &args[0] {
                Object::Number(n) => *n,
                _ => return new_error(ctx.pos.clone(), "collections.range expects number"),
            };
            (0.0, end, 1.0)
        }
        2 => {
            let start = match &args[0] {
                Object::Number(n) => *n,
                _ => return new_error(ctx.pos.clone(), "collections.range expects number"),
            };
            let end = match &args[1] {
                Object::Number(n) => *n,
                _ => return new_error(ctx.pos.clone(), "collections.range expects number"),
            };
            (start, end, 1.0)
        }
        _ => {
            let start = match &args[0] {
                Object::Number(n) => *n,
                _ => return new_error(ctx.pos.clone(), "collections.range expects number"),
            };
            let end = match &args[1] {
                Object::Number(n) => *n,
                _ => return new_error(ctx.pos.clone(), "collections.range expects number"),
            };
            let step = match &args[2] {
                Object::Number(n) => *n,
                _ => return new_error(ctx.pos.clone(), "collections.range expects number"),
            };
            (start, end, step)
        }
    };
    if step == 0.0 {
        return new_error(ctx.pos.clone(), "collections.range step cannot be zero");
    }
    let mut out = Vec::new();
    if step > 0.0 {
        let mut i = start;
        while i < end {
            out.push(num_obj(i));
            i += step;
        }
    } else {
        let mut i = start;
        while i > end {
            out.push(num_obj(i));
            i += step;
        }
    }
    array(out)
}

// ---------------------------------------------------------------------------
// process: argv / pid / cwd / exit / hrtime, etc.
// ---------------------------------------------------------------------------
