use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{num_obj, str_obj, CallContext, HashData, Object};

pub(crate) const PROMETHEUS_STATE_KEY: &str = "__prometheus_state__";

pub(crate) fn prometheus_module() -> Object {
    module(vec![(
        "create",
        native("prometheus.create", prometheus_create),
    )])
}

pub(crate) fn prometheus_create(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    // metrics: Hash mapping name -> Number
    let metrics = Rc::new(RefCell::new(HashData::default()));
    let instance = Rc::new(RefCell::new(HashData::default()));
    instance
        .borrow_mut()
        .set(PROMETHEUS_STATE_KEY, Object::Hash(metrics.clone()));

    let m = metrics.clone();
    instance.borrow_mut().set(
        "inc",
        native("prometheus.inc", move |ctx, args| {
            let name = match required_string(ctx, "prometheus.inc", args, 0, "name") {
                Ok(n) => n,
                Err(e) => return e,
            };
            let mut g = m.borrow_mut();
            let current = match g.get(&name) {
                Some(Object::Number(n)) => *n,
                _ => 0.0,
            };
            g.set(name, num_obj(current + 1.0));
            Object::Undefined
        }),
    );

    let m = metrics.clone();
    instance.borrow_mut().set(
        "set",
        native("prometheus.set", move |ctx, args| {
            let name = match required_string(ctx, "prometheus.set", args, 0, "name") {
                Ok(n) => n,
                Err(e) => return e,
            };
            let value = match required_number(ctx, "prometheus.set", args, 1, "value") {
                Ok(v) => v,
                Err(e) => return e,
            };
            m.borrow_mut().set(name, num_obj(value));
            Object::Undefined
        }),
    );

    let m = metrics.clone();
    instance.borrow_mut().set(
        "get",
        native("prometheus.get", move |ctx, args| {
            let name = match required_string(ctx, "prometheus.get", args, 0, "name") {
                Ok(n) => n,
                Err(e) => return e,
            };
            match m.borrow().get(&name).cloned() {
                Some(Object::Number(n)) => num_obj(n),
                _ => num_obj(0.0),
            }
        }),
    );

    let m = metrics.clone();
    instance.borrow_mut().set(
        "snapshot",
        native("prometheus.snapshot", move |_ctx, _args| {
            let g = m.borrow();
            let mut entries: Vec<Object> = Vec::with_capacity(g.entries.len());
            for (k, v) in &g.entries {
                let entry = Rc::new(RefCell::new(HashData::default()));
                entry.borrow_mut().set("name", str_obj(k.clone()));
                entry.borrow_mut().set("value", v.clone());
                entries.push(Object::Hash(entry));
            }
            array(entries)
        }),
    );

    Object::Hash(instance)
}

// ---------------------------------------------------------------------------
// highlight: terminal syntax highlighting subset (@std/highlight)
// ---------------------------------------------------------------------------
