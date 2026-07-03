use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, HashData, Object};

pub(crate) fn cache_module() -> Object {
    module(vec![("create", native("cache.create", cache_create))])
}

/// Cache state is stored as a Hash carrying a hidden marker key whose value
/// is a Hash mapping keys -> { value, expireAt } records. This keeps the
/// cache shareable through the existing object model without adding a new
/// Object variant.
const CACHE_STATE_KEY: &str = "__cache_state__";

pub(crate) fn cache_create(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    // The backing store: a Hash mapping key -> Hash{value, expireAtMs?}.
    let store = Rc::new(RefCell::new(HashData::default()));
    let instance = Rc::new(RefCell::new(HashData::default()));
    instance
        .borrow_mut()
        .set(CACHE_STATE_KEY, Object::Hash(store.clone()));

    let store_for_set = store.clone();
    instance.borrow_mut().set(
        "set",
        native("cache.set", move |ctx, args| {
            let key = match required_string(ctx, "cache.set", args, 0, "key") {
                Ok(k) => k,
                Err(e) => return e,
            };
            let value = match args.get(1) {
                Some(v) => v.clone(),
                None => return new_error(ctx.pos.clone(), "cache.set requires key and value"),
            };
            let ttl_ms = match args.get(2) {
                Some(Object::Number(n)) if *n > 0.0 => Some(*n as u64),
                _ => None,
            };
            let entry = Rc::new(RefCell::new(HashData::default()));
            entry.borrow_mut().set("value", value);
            if let Some(ms) = ttl_ms {
                entry
                    .borrow_mut()
                    .set("expireAtMs", num_obj(now_millis() + ms as f64));
            }
            store_for_set.borrow_mut().set(key, Object::Hash(entry));
            Object::Undefined
        }),
    );

    let store_for_get = store.clone();
    instance.borrow_mut().set(
        "get",
        native("cache.get", move |ctx, args| {
            let key = match required_string(ctx, "cache.get", args, 0, "key") {
                Ok(k) => k,
                Err(e) => return e,
            };
            let entry = match store_for_get.borrow().get(&key).cloned() {
                Some(Object::Hash(h)) => h,
                _ => return Object::Undefined,
            };
            // entry is an owned Rc<RefCell<HashData>>; safe to borrow here.
            let expired = match entry.borrow().get("expireAtMs").cloned() {
                Some(Object::Number(expire)) => now_millis() > expire,
                _ => false,
            };
            if expired {
                store_for_get.borrow_mut().remove(&key);
                return Object::Undefined;
            }
            let value = entry
                .borrow()
                .get("value")
                .cloned()
                .unwrap_or(Object::Undefined);
            value
        }),
    );

    let store_for_has = store.clone();
    instance.borrow_mut().set(
        "has",
        native("cache.has", move |ctx, args| {
            let key = match required_string(ctx, "cache.has", args, 0, "key") {
                Ok(k) => k,
                Err(e) => return e,
            };
            match store_for_has.borrow().get(&key).cloned() {
                Some(Object::Hash(entry)) => {
                    if let Some(Object::Number(expire)) = entry.borrow().get("expireAtMs").cloned()
                    {
                        if now_millis() > expire {
                            return bool_obj(false);
                        }
                    }
                    bool_obj(true)
                }
                _ => bool_obj(false),
            }
        }),
    );

    let store_for_delete = store.clone();
    instance.borrow_mut().set(
        "delete",
        native("cache.delete", move |ctx, args| {
            let key = match required_string(ctx, "cache.delete", args, 0, "key") {
                Ok(k) => k,
                Err(e) => return e,
            };
            store_for_delete.borrow_mut().remove(&key);
            Object::Undefined
        }),
    );

    let store_for_clear = store.clone();
    instance.borrow_mut().set(
        "clear",
        native("cache.clear", move |_ctx, _args| {
            store_for_clear.borrow_mut().entries.clear();
            Object::Undefined
        }),
    );

    let store_for_size = store.clone();
    instance.borrow_mut().set(
        "size",
        native("cache.size", move |_ctx, _args| {
            num_obj(store_for_size.borrow().entries.len() as f64)
        }),
    );

    let store_for_keys = store.clone();
    instance.borrow_mut().set(
        "keys",
        native("cache.keys", move |_ctx, _args| {
            let keys: Vec<Object> = store_for_keys
                .borrow()
                .entries
                .iter()
                .map(|(k, _)| str_obj(k.clone()))
                .collect();
            array(keys)
        }),
    );

    Object::Hash(instance)
}
