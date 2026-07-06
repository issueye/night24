use std::cell::RefCell;
use std::rc::Rc;

use crate::object::*;

use super::super::expressions::apply_function;
use super::{as_num, native, normalize_index, FnPtr};

pub(super) fn array_global() -> Object {
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
                Some(value) => {
                    super::super::iterator::collect_iterable(value, ctx.env, ctx.pos.clone())
                }
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
