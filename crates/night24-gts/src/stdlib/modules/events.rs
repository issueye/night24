use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, CallContext, HashData, Object};

pub(crate) fn events_module() -> Object {
    module(vec![(
        "EventEmitter",
        native("events.EventEmitter", events_create),
    )])
}

type ListenerList = Vec<(usize, Object, bool)>;

pub(crate) fn events_create(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    let store: Rc<RefCell<HashMap<String, ListenerList>>> = Rc::new(RefCell::new(HashMap::new()));
    let next_id = Rc::new(std::cell::Cell::new(0usize));
    let emitter = Rc::new(RefCell::new(HashData::default()));

    let s = store.clone();
    let n = next_id.clone();
    emitter.borrow_mut().set(
        "on",
        native("events.on", move |ctx, args| {
            events_add(ctx, args, &s, &n, false)
        }),
    );
    let s = store.clone();
    let n = next_id.clone();
    emitter.borrow_mut().set(
        "once",
        native("events.once", move |ctx, args| {
            events_add(ctx, args, &s, &n, true)
        }),
    );
    let s = store.clone();
    emitter.borrow_mut().set(
        "off",
        native("events.off", move |ctx, args| events_remove(ctx, args, &s)),
    );
    let s = store.clone();
    let e = emitter.clone();
    emitter.borrow_mut().set(
        "emit",
        native("events.emit", move |ctx, args| {
            events_emit(ctx, args, &s, &e)
        }),
    );
    let s = store.clone();
    emitter.borrow_mut().set(
        "listeners",
        native("events.listeners", move |ctx, args| {
            events_listeners(ctx, args, &s)
        }),
    );
    let s = store.clone();
    emitter.borrow_mut().set(
        "listenerCount",
        native("events.listenerCount", move |ctx, args| {
            events_count(ctx, args, &s)
        }),
    );
    let s = store.clone();
    emitter.borrow_mut().set(
        "removeAllListeners",
        native("events.removeAllListeners", move |ctx, args| {
            events_remove_all(ctx, args, &s)
        }),
    );
    Object::Hash(emitter)
}

pub(crate) fn events_add(
    ctx: &mut CallContext,
    args: &[Object],
    store: &Rc<RefCell<HashMap<String, ListenerList>>>,
    next_id: &Rc<std::cell::Cell<usize>>,
    once: bool,
) -> Object {
    let event = match required_string(ctx, "events.add", args, 0, "event") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let listener = match args.get(1) {
        Some(v @ (Object::Function(_) | Object::Builtin(_) | Object::Closure(_))) => v.clone(),
        Some(_) => return new_error(ctx.pos.clone(), "events: listener must be a function"),
        None => return new_error(ctx.pos.clone(), "events requires listener"),
    };
    let id = next_id.get();
    next_id.set(id + 1);
    store
        .borrow_mut()
        .entry(event.as_str().to_string())
        .or_default()
        .push((id, listener, once));
    Object::Undefined
}

pub(crate) fn events_remove(
    ctx: &mut CallContext,
    args: &[Object],
    store: &Rc<RefCell<HashMap<String, ListenerList>>>,
) -> Object {
    let event = match required_string(ctx, "events.off", args, 0, "event") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let listener = match args.get(1) {
        Some(v @ (Object::Function(_) | Object::Builtin(_) | Object::Closure(_))) => v.clone(),
        _ => return new_error(ctx.pos.clone(), "events.off requires listener"),
    };
    if let Some(list) = store.borrow_mut().get_mut(&event) {
        let key = listener.inspect();
        if let Some(pos) = list.iter().position(|(_, f, _)| f.inspect() == key) {
            list.remove(pos);
        }
        if list.is_empty() {
            store.borrow_mut().remove(event.as_str());
        }
    }
    Object::Undefined
}

pub(crate) fn events_emit(
    ctx: &mut CallContext,
    args: &[Object],
    store: &Rc<RefCell<HashMap<String, ListenerList>>>,
    emitter: &Rc<RefCell<HashData>>,
) -> Object {
    let event = match required_string(ctx, "events.emit", args, 0, "event") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let call_args: Vec<Object> = args.iter().skip(1).cloned().collect();
    // Snapshot listeners (including once) so they fire this emit, then remove
    // once listeners from the store afterwards (matching Go semantics).
    let snapshot: Vec<Object> = {
        let mut s = store.borrow_mut();
        let list = match s.get_mut(&event) {
            Some(l) => l,
            None => return bool_obj(false),
        };
        let snap: Vec<Object> = list.iter().map(|(_, f, _)| f.clone()).collect();
        list.retain(|(_, _, once)| !once);
        snap
    };
    let emitter_obj = Object::Hash(emitter.clone());
    for listener in &snapshot {
        let result = crate::evaluator::expressions::apply_function(
            listener,
            ctx.env,
            &call_args,
            Some(emitter_obj.clone()),
            ctx.pos.clone(),
        );
        if result.is_runtime_error() {
            return result;
        }
    }
    bool_obj(true)
}

pub(crate) fn events_listeners(
    ctx: &mut CallContext,
    args: &[Object],
    store: &Rc<RefCell<HashMap<String, ListenerList>>>,
) -> Object {
    let event = match required_string(ctx, "events.listeners", args, 0, "event") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let fns: Vec<Object> = store
        .borrow()
        .get(&event)
        .map(|list| list.iter().map(|(_, f, _)| f.clone()).collect())
        .unwrap_or_default();
    array(fns)
}

pub(crate) fn events_count(
    ctx: &mut CallContext,
    args: &[Object],
    store: &Rc<RefCell<HashMap<String, ListenerList>>>,
) -> Object {
    let event = match required_string(ctx, "events.listenerCount", args, 0, "event") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let count = store.borrow().get(&event).map(|l| l.len()).unwrap_or(0);
    num_obj(count as f64)
}

pub(crate) fn events_remove_all(
    ctx: &mut CallContext,
    args: &[Object],
    store: &Rc<RefCell<HashMap<String, ListenerList>>>,
) -> Object {
    match args.first() {
        None | Some(Object::Undefined) => store.borrow_mut().clear(),
        Some(Object::String(event)) => {
            store.borrow_mut().remove(event.as_str());
        }
        Some(_) => {
            return new_error(
                ctx.pos.clone(),
                "events.removeAllListeners: event must be a string",
            )
        }
    }
    Object::Undefined
}

// ---------------------------------------------------------------------------
// jwt: HS256 sign/verify/decode using the self-contained hmac+sha256.
// ---------------------------------------------------------------------------
