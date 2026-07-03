//! Shared `Symbol.iterator` protocol helpers.

use std::cell::RefCell;
use std::rc::Rc;

use crate::ast::Position;
use crate::object::*;

use super::expressions::apply_function;
use super::methods::{get_property, native_builtin};

/// The runtime key used for `Symbol.iterator`.
///
/// GTS objects are string-keyed today, so the well-known symbol is represented
/// as a stable internal string. Computed property access makes it visible as
/// `obj[Symbol.iterator]`.
pub const SYMBOL_ITERATOR_KEY: &str = "@@iterator";

pub fn symbol_global() -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    hash.borrow_mut()
        .set("iterator", str_obj(SYMBOL_ITERATOR_KEY));
    Object::Hash(hash)
}

pub fn default_iterator_method(receiver: Object, name: &'static str) -> Object {
    let fallback_receiver = receiver.clone();
    native_builtin(
        name,
        move |ctx, _args| {
            values_iterator_from(&ctx.receiver.clone().unwrap_or(fallback_receiver.clone()))
        },
        Some(receiver),
    )
}

pub fn values_iterator_from(value: &Object) -> Object {
    let values = match value {
        Object::Array(a) => a.borrow().elements.clone(),
        Object::Hash(h) => h
            .borrow()
            .entries
            .iter()
            .filter(|(k, _)| k.as_str() != "__call" && k.as_str() != SYMBOL_ITERATOR_KEY)
            .map(|(_, v)| v.clone())
            .collect(),
        Object::String(s) => s.chars().map(|c| str_obj(c.to_string())).collect(),
        Object::Map(m) => m
            .borrow()
            .entries
            .iter()
            .map(|(_, _, value)| value.clone())
            .collect(),
        Object::Set(s) => s
            .borrow()
            .entries
            .iter()
            .map(|(_, value)| value.clone())
            .collect(),
        _ => Vec::new(),
    };
    iterator_from_values(values)
}

pub fn iterator_from_values(values: Vec<Object>) -> Object {
    let state = Rc::new(RefCell::new(IteratorState { values, index: 0 }));
    let hash = Rc::new(RefCell::new(HashData::default()));

    let next_state = state.clone();
    hash.borrow_mut().set(
        "next",
        native_builtin(
            "Iterator.next",
            move |_ctx, _args| {
                let mut state = next_state.borrow_mut();
                if state.index >= state.values.len() {
                    return iterator_result(Object::Undefined, true);
                }
                let value = state.values[state.index].clone();
                state.index += 1;
                iterator_result(value, false)
            },
            None,
        ),
    );

    let self_ref = Rc::new(RefCell::new(Object::Undefined));
    let return_self = self_ref.clone();
    hash.borrow_mut().set(
        SYMBOL_ITERATOR_KEY,
        native_builtin(
            "Iterator.Symbol.iterator",
            move |_ctx, _args| return_self.borrow().clone(),
            None,
        ),
    );

    let iterator = Object::Hash(hash);
    *self_ref.borrow_mut() = iterator.clone();
    iterator
}

pub fn get_iterator(value: &Object, env: &EnvRef, pos: Position) -> Object {
    let method = get_property(value, SYMBOL_ITERATOR_KEY, pos.clone());
    if method.is_runtime_error() {
        return method;
    }
    if matches!(method, Object::Undefined) {
        return new_error(
            pos,
            format!("TypeError: {} is not iterable", value.type_tag()),
        );
    }
    apply_function(&method, env, &[], Some(value.clone()), pos)
}

pub fn iterator_next(iterator: &Object, env: &EnvRef, pos: Position) -> Object {
    let next = get_property(iterator, "next", pos.clone());
    if next.is_runtime_error() {
        return next;
    }
    if matches!(next, Object::Undefined) {
        return new_error(pos, "TypeError: iterator.next is not a function");
    }
    let result = apply_function(&next, env, &[], Some(iterator.clone()), pos.clone());
    if result.is_runtime_error() {
        return result;
    }
    match result {
        Object::Hash(_) => result,
        other => new_error(
            pos,
            format!("TypeError: iterator.next returned {}", other.type_tag()),
        ),
    }
}

pub fn iterator_done(record: &Object, pos: Position) -> Object {
    get_property(record, "done", pos)
}

pub fn iterator_value(record: &Object, pos: Position) -> Object {
    get_property(record, "value", pos)
}

pub fn collect_iterable(value: &Object, env: &EnvRef, pos: Position) -> Object {
    let iterator = get_iterator(value, env, pos.clone());
    if iterator.is_runtime_error() {
        return iterator;
    }

    let mut values = Vec::new();
    loop {
        let next = iterator_next(&iterator, env, pos.clone());
        if next.is_runtime_error() {
            return next;
        }
        let done = iterator_done(&next, pos.clone());
        if done.is_runtime_error() {
            return done;
        }
        if done.is_truthy() {
            break;
        }
        let value = iterator_value(&next, pos.clone());
        if value.is_runtime_error() {
            return value;
        }
        values.push(value);
    }

    Object::Array(Rc::new(RefCell::new(ArrayData { elements: values })))
}

pub fn get_iterator_index(obj: &Object, key: &Object, pos: Position) -> Option<Object> {
    if key.inspect() == SYMBOL_ITERATOR_KEY {
        return match obj {
            Object::Array(_)
            | Object::Hash(_)
            | Object::String(_)
            | Object::Map(_)
            | Object::Set(_) => Some(get_property(obj, SYMBOL_ITERATOR_KEY, pos)),
            _ => None,
        };
    }
    None
}

fn iterator_result(value: Object, done: bool) -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    {
        let mut h = hash.borrow_mut();
        h.set("value", value);
        h.set("done", Object::Boolean(done));
    }
    Object::Hash(hash)
}

struct IteratorState {
    values: Vec<Object>,
    index: usize,
}
