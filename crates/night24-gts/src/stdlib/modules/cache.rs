use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, Object};

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
    let store = ObjectBuilder::new().into_shared();
    let instance = ObjectBuilder::new()
        .set(CACHE_STATE_KEY, Object::Hash(store.clone()))
        .into_shared();

    let store_for_set = store.clone();
    instance.borrow_mut().set(
        "set",
        native("cache.set", move |ctx, args| {
            let reader = ArgReader::new(ctx, "cache.set", args);
            let key = match reader.required_string(0, "key") {
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
            store_for_set
                .borrow_mut()
                .set(key, cache_entry(value, ttl_ms, now_millis()));
            Object::Undefined
        }),
    );

    let store_for_get = store.clone();
    instance.borrow_mut().set(
        "get",
        native("cache.get", move |ctx, args| {
            let reader = ArgReader::new(ctx, "cache.get", args);
            let key = match reader.required_string(0, "key") {
                Ok(k) => k,
                Err(e) => return e,
            };
            let entry = match store_for_get.borrow().get(&key).cloned() {
                Some(Object::Hash(h)) => h,
                _ => return Object::Undefined,
            };
            // entry is an owned Rc<RefCell<HashData>>; safe to borrow here.
            let expired = cache_entry_is_expired(&entry.borrow(), now_millis());
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
            let reader = ArgReader::new(ctx, "cache.has", args);
            let key = match reader.required_string(0, "key") {
                Ok(k) => k,
                Err(e) => return e,
            };
            match store_for_has.borrow().get(&key).cloned() {
                Some(Object::Hash(entry)) => {
                    if cache_entry_is_expired(&entry.borrow(), now_millis()) {
                        return bool_obj(false);
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
            let reader = ArgReader::new(ctx, "cache.delete", args);
            let key = match reader.required_string(0, "key") {
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

fn cache_entry(value: Object, ttl_ms: Option<u64>, now_ms: f64) -> Object {
    let mut entry = ObjectBuilder::new().set("value", value);
    if let Some(ms) = ttl_ms {
        entry.insert("expireAtMs", num_obj(now_ms + ms as f64));
    }
    entry.build()
}

fn cache_entry_is_expired(entry: &crate::object::HashData, now_ms: f64) -> bool {
    match entry.get("expireAtMs") {
        Some(Object::Number(expire)) => now_ms > *expire,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_ref(object: &Object) -> std::cell::Ref<'_, crate::object::HashData> {
        match object {
            Object::Hash(hash) => hash.borrow(),
            _ => panic!("expected hash object"),
        }
    }

    #[test]
    fn cache_entry_without_ttl_keeps_value_without_expiration() {
        let entry = cache_entry(str_obj("value"), None, 1_000.0);
        let entry = hash_ref(&entry);

        assert_eq!(entry.get("value").cloned(), Some(str_obj("value")));
        assert!(entry.get("expireAtMs").is_none());
        assert!(!cache_entry_is_expired(&entry, 9_999.0));
    }

    #[test]
    fn cache_entry_with_ttl_sets_expiration_from_current_time() {
        let entry = cache_entry(num_obj(24.0), Some(250), 1_000.0);
        let entry = hash_ref(&entry);

        assert_eq!(entry.get("value").cloned(), Some(num_obj(24.0)));
        assert_eq!(entry.get("expireAtMs").cloned(), Some(num_obj(1_250.0)));
    }

    #[test]
    fn cache_entry_expiration_uses_strictly_greater_than() {
        let entry = cache_entry(Object::Undefined, Some(250), 1_000.0);
        let entry = hash_ref(&entry);

        assert!(!cache_entry_is_expired(&entry, 1_250.0));
        assert!(cache_entry_is_expired(&entry, 1_250.1));
    }
}
