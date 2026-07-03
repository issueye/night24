use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, CallContext, HashData, Object};

pub(crate) const RATE_LIMIT_STATE_KEY: &str = "__rate_limit_state__";

pub(crate) fn rate_limit_module() -> Object {
    module(vec![(
        "create",
        native("rateLimit.create", rate_limit_create),
    )])
}

pub(crate) fn rate_limit_create(ctx: &mut CallContext, args: &[Object]) -> Object {
    let mut rate = 10.0_f64;
    let mut capacity = 10.0_f64;
    if let Some(Object::Hash(opts)) = args.first() {
        if let Some(Object::Number(n)) = opts.borrow().get("rate") {
            rate = *n;
        }
        if let Some(Object::Number(n)) = opts.borrow().get("capacity") {
            capacity = *n;
        }
    }
    if rate <= 0.0 || capacity <= 0.0 {
        return new_error(
            ctx.pos.clone(),
            "rateLimit.create: rate and capacity must be positive",
        );
    }
    // State stored as a HashData so it survives inside the object model.
    let state = Rc::new(RefCell::new(HashData::default()));
    state.borrow_mut().set("tokens", num_obj(capacity));
    state.borrow_mut().set("capacity", num_obj(capacity));
    state.borrow_mut().set("rate", num_obj(rate));
    state.borrow_mut().set("lastTimeMs", num_obj(now_millis()));

    let instance = Rc::new(RefCell::new(HashData::default()));
    instance
        .borrow_mut()
        .set(RATE_LIMIT_STATE_KEY, Object::Hash(state.clone()));

    let s = state.clone();
    instance.borrow_mut().set(
        "tryAcquire",
        native("rateLimit.tryAcquire", move |_ctx, _args| {
            let mut g = s.borrow_mut();
            let capacity = match g.get("capacity") {
                Some(Object::Number(n)) => *n,
                _ => capacity_fallback(),
            };
            let rate = match g.get("rate") {
                Some(Object::Number(n)) => *n,
                _ => rate_fallback(),
            };
            let now = now_millis();
            let last = match g.get("lastTimeMs") {
                Some(Object::Number(n)) => *n,
                _ => now,
            };
            let elapsed = ((now - last) / 1000.0).max(0.0);
            let tokens = match g.get("tokens") {
                Some(Object::Number(n)) => (n + elapsed * rate).min(capacity),
                _ => capacity,
            };
            g.set("tokens", num_obj(tokens));
            g.set("lastTimeMs", num_obj(now));
            if tokens >= 1.0 {
                g.set("tokens", num_obj(tokens - 1.0));
                bool_obj(true)
            } else {
                bool_obj(false)
            }
        }),
    );

    let s = state.clone();
    instance.borrow_mut().set(
        "acquire",
        native("rateLimit.acquire", move |_ctx, _args| loop {
            let wait_ms = {
                let mut g = s.borrow_mut();
                let capacity = match g.get("capacity") {
                    Some(Object::Number(n)) => *n,
                    _ => capacity_fallback(),
                };
                let rate = match g.get("rate") {
                    Some(Object::Number(n)) => *n,
                    _ => rate_fallback(),
                };
                let now = now_millis();
                let last = match g.get("lastTimeMs") {
                    Some(Object::Number(n)) => *n,
                    _ => now,
                };
                let elapsed = ((now - last) / 1000.0).max(0.0);
                let tokens = match g.get("tokens") {
                    Some(Object::Number(n)) => (n + elapsed * rate).min(capacity),
                    _ => capacity,
                };
                g.set("tokens", num_obj(tokens));
                g.set("lastTimeMs", num_obj(now));
                if tokens >= 1.0 {
                    g.set("tokens", num_obj(tokens - 1.0));
                    return Object::Undefined;
                }
                (((1.0 - tokens) / rate) * 1000.0).max(0.0) as u64
            };
            if wait_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(wait_ms));
            }
        }),
    );

    let s = state.clone();
    instance.borrow_mut().set(
        "remaining",
        native("rateLimit.remaining", move |_ctx, _args| {
            let g = s.borrow();
            match g.get("tokens") {
                Some(Object::Number(n)) => num_obj(*n),
                _ => num_obj(0.0),
            }
        }),
    );

    Object::Hash(instance)
}

#[inline]
fn capacity_fallback() -> f64 {
    10.0
}

#[inline]
fn rate_fallback() -> f64 {
    10.0
}

// ---------------------------------------------------------------------------
// prometheus: minimal metrics registry (@std/prometheus)
// ---------------------------------------------------------------------------
