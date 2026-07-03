//! Bytecode function-call helpers.
//!
//! The interpreter dispatches `CALL`, but closure invocation lives here so
//! call-frame semantics can grow without making `interp.rs` carry every stage.

use crate::ast::{Position, TypeAnnotation};
use crate::object::{new_error, EnvRef, Object, Promise};
use std::sync::atomic::Ordering;

/// Call a bytecode closure: bind params into a child scope of the closure's
/// home environment, then run the body chunk.
pub fn call_closure(
    c: &crate::bytecode::closure::ClosureData,
    args: &[Object],
    caller_env: &EnvRef,
    pos: Position,
) -> Result<Object, Object> {
    call_closure_with_this(c, args, caller_env, None, pos)
}

pub fn call_closure_with_this(
    c: &crate::bytecode::closure::ClosureData,
    args: &[Object],
    caller_env: &EnvRef,
    this: Option<Object>,
    pos: Position,
) -> Result<Object, Object> {
    call_closure_impl(c, caller_env, args, this, pos)
}

/// Public entry for native -> VM callback (used by `apply_function` when it
/// encounters an `Object::Closure`, e.g. an array `.map(fn)` callback).
pub fn call_closure_object(
    c: std::rc::Rc<crate::bytecode::closure::ClosureData>,
    caller_env: &EnvRef,
    args: &[Object],
    pos: Position,
) -> Object {
    match call_closure_impl(&c, caller_env, args, None, pos) {
        Ok(v) => v,
        Err(e) => e,
    }
}

fn call_closure_impl(
    c: &crate::bytecode::closure::ClosureData,
    caller_env: &EnvRef,
    args: &[Object],
    this: Option<Object>,
    pos: Position,
) -> Result<Object, Object> {
    let proto = &c.proto;
    let scope = crate::object::Environment::child(&c.home_env);
    if let Some(t) = this {
        scope.borrow_mut().this = Some(t);
    } else if !proto.lexical_this {
        scope.borrow_mut().this = None;
    }

    if let Err(e) = crate::evaluator::expressions::bind_params(
        &scope,
        caller_env,
        &proto.params,
        args,
        pos.clone(),
    ) {
        if proto.is_async {
            let promise = Promise::new();
            promise.reject(e);
            return Ok(Object::Promise(promise));
        }
        return Err(e);
    }
    bind_upvalues_into_scope(c, &scope);
    let _frame = crate::bytecode::frame::CallFrame::from_bound_env(proto.clone(), &scope, 0);

    let chunk = match proto.chunk.borrow().clone() {
        Some(c) => c,
        None => {
            let error = new_error(pos, "VMError: function body not compiled");
            if proto.is_async {
                let promise = Promise::new();
                promise.reject(error);
                return Ok(Object::Promise(promise));
            }
            return Err(error);
        }
    };
    let result = super::interp::interpret_with_upvalues(&chunk, &scope, c.upvalues.clone());
    flush_scope_to_upvalues(c, &scope);
    if result.is_runtime_error() {
        if proto.is_async {
            let promise = Promise::new();
            promise.reject(result);
            return Ok(Object::Promise(promise));
        }
        Err(result)
    } else if let Some(return_t) = &proto.return_t {
        if let Err(e) = check_return_type(&result, return_t, caller_env, pos) {
            if proto.is_async {
                let promise = Promise::new();
                promise.reject(e);
                return Ok(Object::Promise(promise));
            }
            return Err(e);
        }
        if proto.is_async {
            let promise = Promise::new();
            promise.resolve(result);
            return Ok(Object::Promise(promise));
        }
        Ok(result)
    } else {
        if proto.is_async {
            let promise = Promise::new();
            promise.resolve(result);
            return Ok(Object::Promise(promise));
        }
        Ok(result)
    }
}

fn check_return_type(
    value: &Object,
    return_t: &TypeAnnotation,
    caller_env: &EnvRef,
    pos: Position,
) -> Result<(), Object> {
    if !caller_env.borrow().vm.type_check.load(Ordering::Relaxed)
        || super::interp::value_matches_type_annotation(value, return_t)
    {
        return Ok(());
    }
    Err(new_error(
        pos,
        format!(
            "TypeError: cannot return {} from function returning {}",
            value.type_tag(),
            return_t
        ),
    ))
}

fn bind_upvalues_into_scope(c: &crate::bytecode::closure::ClosureData, scope: &EnvRef) {
    for (name, upvalue) in c.upvalue_names.iter().zip(c.upvalues.iter()) {
        if let Some(value) = upvalue.get(&[]) {
            scope.borrow_mut().set_here(name.clone(), value);
        }
    }
}

fn flush_scope_to_upvalues(c: &crate::bytecode::closure::ClosureData, scope: &EnvRef) {
    for (name, upvalue) in c.upvalue_names.iter().zip(c.upvalues.iter()) {
        let Some(value) = scope
            .borrow()
            .bindings
            .get(name)
            .map(|binding| binding.value.clone())
        else {
            continue;
        };
        let mut empty_slots = [];
        upvalue.set(&mut empty_slots, value);
    }
}
