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
    let emitter = ObjectBuilder::new().into_shared();

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
    let reader = ArgReader::new(ctx, "events.add", args);
    let event = match reader.required_string(0, "event") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let listener = match reader.required_callable(1, "listener") {
        Ok(listener) => listener,
        Err(e) => return e,
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
    let reader = ArgReader::new(ctx, "events.off", args);
    let event = match reader.required_string(0, "event") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let listener = match reader.required_callable(1, "listener") {
        Ok(listener) => listener,
        Err(_) => return new_error(ctx.pos.clone(), "events.off requires listener"),
    };
    let remove_empty = {
        let mut store = store.borrow_mut();
        if let Some(list) = store.get_mut(&event) {
            let key = listener.inspect();
            if let Some(pos) = list.iter().position(|(_, f, _)| f.inspect() == key) {
                list.remove(pos);
            }
            list.is_empty()
        } else {
            false
        }
    };
    if remove_empty {
        store.borrow_mut().remove(event.as_str());
    }
    Object::Undefined
}

pub(crate) fn events_emit(
    ctx: &mut CallContext,
    args: &[Object],
    store: &Rc<RefCell<HashMap<String, ListenerList>>>,
    emitter: &Rc<RefCell<HashData>>,
) -> Object {
    let reader = ArgReader::new(ctx, "events.emit", args);
    let event = match reader.required_string(0, "event") {
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
    let reader = ArgReader::new(ctx, "events.listeners", args);
    let event = match reader.required_string(0, "event") {
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
    let reader = ArgReader::new(ctx, "events.listenerCount", args);
    let event = match reader.required_string(0, "event") {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Position;
    use crate::object::{str_obj, Environment, VirtualMachine};

    fn test_context(env: &crate::object::EnvRef) -> CallContext<'_> {
        CallContext::new(env, Position::default())
    }

    fn listener(name: &str) -> Object {
        native(name, |_ctx, _args| Object::Undefined)
    }

    fn store() -> Rc<RefCell<HashMap<String, ListenerList>>> {
        Rc::new(RefCell::new(HashMap::new()))
    }

    fn count_value(object: Object) -> f64 {
        match object {
            Object::Number(value) => value,
            _ => panic!("expected number"),
        }
    }

    fn array_len(object: Object) -> usize {
        match object {
            Object::Array(array) => array.borrow().elements.len(),
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn listener_count_and_listeners_track_store_state() {
        let env = Environment::new_root(VirtualMachine::new());
        let mut ctx = test_context(&env);
        let store = store();
        let next_id = Rc::new(std::cell::Cell::new(0usize));
        let first = listener("events.test.first");
        let second = listener("events.test.second");

        assert_eq!(
            events_add(
                &mut ctx,
                &[str_obj("ready"), first.clone()],
                &store,
                &next_id,
                false,
            ),
            Object::Undefined
        );
        assert_eq!(
            events_add(
                &mut ctx,
                &[str_obj("ready"), second.clone()],
                &store,
                &next_id,
                false,
            ),
            Object::Undefined
        );

        assert_eq!(
            count_value(events_count(&mut ctx, &[str_obj("ready")], &store)),
            2.0
        );
        assert_eq!(
            array_len(events_listeners(&mut ctx, &[str_obj("ready")], &store)),
            2
        );
        assert_eq!(
            count_value(events_count(&mut ctx, &[str_obj("missing")], &store)),
            0.0
        );
    }

    #[test]
    fn remove_deletes_one_matching_listener_then_cleans_empty_event() {
        let env = Environment::new_root(VirtualMachine::new());
        let mut ctx = test_context(&env);
        let store = store();
        let next_id = Rc::new(std::cell::Cell::new(0usize));
        let first = listener("events.test.first");
        let second = listener("events.test.second");

        events_add(
            &mut ctx,
            &[str_obj("ready"), first.clone()],
            &store,
            &next_id,
            false,
        );
        events_add(
            &mut ctx,
            &[str_obj("ready"), second.clone()],
            &store,
            &next_id,
            false,
        );

        assert_eq!(
            events_remove(&mut ctx, &[str_obj("ready"), first], &store),
            Object::Undefined
        );
        assert_eq!(
            count_value(events_count(&mut ctx, &[str_obj("ready")], &store)),
            1.0
        );
        assert_eq!(
            array_len(events_listeners(&mut ctx, &[str_obj("ready")], &store)),
            1
        );

        events_remove(&mut ctx, &[str_obj("ready"), second], &store);

        assert_eq!(
            count_value(events_count(&mut ctx, &[str_obj("ready")], &store)),
            0.0
        );
        assert!(!store.borrow().contains_key("ready"));
    }

    #[test]
    fn remove_all_keeps_global_and_event_specific_semantics() {
        let env = Environment::new_root(VirtualMachine::new());
        let mut ctx = test_context(&env);
        let store = store();
        let next_id = Rc::new(std::cell::Cell::new(0usize));

        events_add(
            &mut ctx,
            &[str_obj("ready"), listener("events.test.ready")],
            &store,
            &next_id,
            false,
        );
        events_add(
            &mut ctx,
            &[str_obj("close"), listener("events.test.close")],
            &store,
            &next_id,
            false,
        );

        events_remove_all(&mut ctx, &[str_obj("ready")], &store);

        assert_eq!(
            count_value(events_count(&mut ctx, &[str_obj("ready")], &store)),
            0.0
        );
        assert_eq!(
            count_value(events_count(&mut ctx, &[str_obj("close")], &store)),
            1.0
        );

        events_remove_all(&mut ctx, &[], &store);

        assert_eq!(store.borrow().len(), 0);
    }
}

// ---------------------------------------------------------------------------
// jwt: HS256 sign/verify/decode using the self-contained hmac+sha256.
// ---------------------------------------------------------------------------
