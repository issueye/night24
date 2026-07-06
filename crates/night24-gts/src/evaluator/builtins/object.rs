use std::cell::RefCell;
use std::rc::Rc;

use crate::object::*;

use super::{native, FnPtr};

pub(super) fn object_global() -> Object {
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
