use std::cell::RefCell;
use std::rc::Rc;

use crate::object::*;

use super::super::expressions::apply_function;
use super::FnPtr;

pub(super) fn promise_constructor(ctx: &mut CallContext, args: &[Object]) -> Object {
    let promise = Promise::new();
    if let Some(executor) = args.first() {
        let p2 = promise.clone();
        let p3 = promise.clone();
        let resolve_fn: FnPtr = Rc::new(move |_c, a| {
            if let Some(v) = a.first() {
                p2.resolve(v.clone());
            } else {
                p2.resolve(Object::Undefined);
            }
            Object::Undefined
        });
        let reject_fn: FnPtr = Rc::new(move |_c, a| {
            if let Some(v) = a.first() {
                p3.reject(v.clone());
            } else {
                p3.reject(Object::Undefined);
            }
            Object::Undefined
        });
        let resolve = Object::Builtin(Rc::new(Builtin {
            name: "resolve".into(),
            func: resolve_fn,
            extra: None,
        }));
        let reject = Object::Builtin(Rc::new(Builtin {
            name: "reject".into(),
            func: reject_fn,
            extra: None,
        }));
        let vm = ctx.vm();
        let _ = apply_function(executor, ctx.env, &[resolve, reject], None, ctx.pos.clone());
        let _ = vm;
    }
    Object::Promise(promise)
}

pub(super) fn attach_promise_statics(vm: &Rc<VirtualMachine>) {
    let hash = Rc::new(RefCell::new(HashData::default()));
    hash.borrow_mut().set(
        "__call",
        vm.get_global("Promise").unwrap_or(Object::Undefined),
    );
    let resolve_fn: FnPtr = Rc::new(|_ctx, args| {
        let promise = Promise::new();
        promise.resolve(args.first().cloned().unwrap_or(Object::Undefined));
        Object::Promise(promise)
    });
    hash.borrow_mut().set(
        "resolve",
        Object::Builtin(Rc::new(Builtin {
            name: "Promise.resolve".into(),
            func: resolve_fn,
            extra: None,
        })),
    );
    let reject_fn: FnPtr = Rc::new(|_ctx, args| {
        let promise = Promise::new();
        promise.reject(args.first().cloned().unwrap_or(Object::Undefined));
        Object::Promise(promise)
    });
    hash.borrow_mut().set(
        "reject",
        Object::Builtin(Rc::new(Builtin {
            name: "Promise.reject".into(),
            func: reject_fn,
            extra: None,
        })),
    );
    let all_fn: FnPtr = Rc::new(|_ctx, args| {
        let promise = Promise::new();
        match args.first() {
            Some(Object::Array(arr)) => {
                let items: Vec<Object> = arr.borrow_mut().elements.clone();
                let total = items.len();
                if total == 0 {
                    promise.resolve(Object::Array(Rc::new(RefCell::new(ArrayData::default()))));
                    return Object::Promise(promise);
                }
                let results: Rc<RefCell<Vec<Option<Object>>>> =
                    Rc::new(RefCell::new(vec![None; total]));
                let remaining = Rc::new(std::sync::atomic::AtomicUsize::new(total));

                let record_result = |i: usize,
                                     value: Object,
                                     results: &Rc<RefCell<Vec<Option<Object>>>>,
                                     remaining: &Rc<std::sync::atomic::AtomicUsize>,
                                     promise: &Promise| {
                    results.borrow_mut()[i] = Some(value);
                    if remaining.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                        let collected: Vec<Object> = results
                            .borrow_mut()
                            .iter()
                            .map(|o| o.clone().unwrap_or(Object::Undefined))
                            .collect();
                        promise.resolve(Object::Array(Rc::new(RefCell::new(ArrayData {
                            elements: collected,
                        }))));
                    }
                };

                for (i, item) in items.into_iter().enumerate() {
                    match item {
                        Object::Promise(p) => {
                            let v = p.wait();
                            if p.state() == PromiseState::Rejected {
                                promise.reject(v);
                                return Object::Promise(promise);
                            }
                            record_result(i, v, &results, &remaining, &promise);
                        }
                        other => {
                            record_result(i, other, &results, &remaining, &promise);
                        }
                    }
                }
            }
            _ => {
                promise.resolve(Object::Array(Rc::new(RefCell::new(ArrayData::default()))));
            }
        }
        Object::Promise(promise)
    });
    hash.borrow_mut().set(
        "all",
        Object::Builtin(Rc::new(Builtin {
            name: "Promise.all".into(),
            func: all_fn,
            extra: None,
        })),
    );

    let race_fn: FnPtr = Rc::new(|_ctx, args| {
        let promise = Promise::new();
        match args.first() {
            Some(Object::Array(arr)) => {
                let items: Vec<Object> = arr.borrow_mut().elements.clone();
                if items.is_empty() {
                    return Object::Promise(promise);
                }
                let settled = Rc::new(std::sync::atomic::AtomicBool::new(false));
                #[allow(clippy::never_loop)]
                for item in items {
                    match item {
                        Object::Promise(p) => {
                            let v = p.wait();
                            if !settled.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                if p.state() == PromiseState::Rejected {
                                    promise.reject(v);
                                } else {
                                    promise.resolve(v);
                                }
                            }
                            break;
                        }
                        other => {
                            if !settled.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                promise.resolve(other);
                            }
                            break;
                        }
                    }
                }
            }
            _ => {
                promise.resolve(Object::Undefined);
            }
        }
        Object::Promise(promise)
    });
    hash.borrow_mut().set(
        "race",
        Object::Builtin(Rc::new(Builtin {
            name: "Promise.race".into(),
            func: race_fn,
            extra: None,
        })),
    );

    let all_settled_fn: FnPtr = Rc::new(|_ctx, args| {
        let promise = Promise::new();
        match args.first() {
            Some(Object::Array(arr)) => {
                let items: Vec<Object> = arr.borrow_mut().elements.clone();
                if items.is_empty() {
                    promise.resolve(Object::Array(Rc::new(RefCell::new(ArrayData::default()))));
                    return Object::Promise(promise);
                }

                let mut results = Vec::with_capacity(items.len());
                for item in items {
                    let result_obj = match item {
                        Object::Promise(p) => {
                            let value = p.wait();
                            let result_hash = Rc::new(RefCell::new(HashData::default()));
                            if p.state() == PromiseState::Rejected {
                                result_hash
                                    .borrow_mut()
                                    .set("status".to_string(), str_obj("rejected".to_string()));
                                result_hash.borrow_mut().set("reason".to_string(), value);
                            } else {
                                result_hash
                                    .borrow_mut()
                                    .set("status".to_string(), str_obj("fulfilled".to_string()));
                                result_hash.borrow_mut().set("value".to_string(), value);
                            }
                            Object::Hash(result_hash)
                        }
                        other => {
                            let result_hash = Rc::new(RefCell::new(HashData::default()));
                            result_hash
                                .borrow_mut()
                                .set("status".to_string(), str_obj("fulfilled".to_string()));
                            result_hash.borrow_mut().set("value".to_string(), other);
                            Object::Hash(result_hash)
                        }
                    };
                    results.push(result_obj);
                }
                promise.resolve(Object::Array(Rc::new(RefCell::new(ArrayData {
                    elements: results,
                }))));
            }
            _ => {
                promise.resolve(Object::Array(Rc::new(RefCell::new(ArrayData::default()))));
            }
        }
        Object::Promise(promise)
    });
    hash.borrow_mut().set(
        "allSettled",
        Object::Builtin(Rc::new(Builtin {
            name: "Promise.allSettled".into(),
            func: all_settled_fn,
            extra: None,
        })),
    );

    vm.set_global("Promise", Object::Hash(hash));
}

fn active_promise(ctx: &CallContext) -> Option<Rc<Promise>> {
    match &ctx.receiver {
        Some(Object::Promise(p)) => Some(p.clone()),
        _ => None,
    }
}

pub fn promise_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "then" => Some(prom_then),
        "catch" => Some(prom_catch),
        "finally" => Some(prom_finally),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}

fn prom_then(ctx: &mut CallContext, args: &[Object]) -> Object {
    let p = match active_promise(ctx) {
        Some(p) => p,
        None => return Object::Undefined,
    };
    let on_fulfilled = args.first().cloned();
    let next = Promise::new();
    let env = ctx.env.clone();
    let pos = ctx.pos.clone();
    let next_for_continuation = next.clone();
    p.add_continuation(Box::new(move |state, result| {
        if state == PromiseState::Rejected {
            next_for_continuation.reject(result);
            return;
        }
        match &on_fulfilled {
            Some(f) => {
                let r = apply_function(f, &env, &[result], None, pos.clone());
                resolve_chained_promise(&next_for_continuation, r);
            }
            None => next_for_continuation.resolve(result),
        }
    }));
    Object::Promise(next)
}

fn prom_catch(ctx: &mut CallContext, args: &[Object]) -> Object {
    let p = match active_promise(ctx) {
        Some(p) => p,
        None => return Object::Undefined,
    };
    let on_reject = args.first().cloned();
    let next = Promise::new();
    let env = ctx.env.clone();
    let pos = ctx.pos.clone();
    let next_for_continuation = next.clone();
    p.add_continuation(Box::new(move |state, result| {
        if state == PromiseState::Fulfilled {
            next_for_continuation.resolve(result);
            return;
        }
        match &on_reject {
            Some(f) => {
                let r = apply_function(f, &env, &[result], None, pos.clone());
                resolve_chained_promise(&next_for_continuation, r);
            }
            None => next_for_continuation.reject(result),
        }
    }));
    Object::Promise(next)
}

fn prom_finally(ctx: &mut CallContext, args: &[Object]) -> Object {
    let p = match active_promise(ctx) {
        Some(p) => p,
        None => return Object::Undefined,
    };
    let on_finally = args.first().cloned();
    let next = Promise::new();
    let env = ctx.env.clone();
    let pos = ctx.pos.clone();
    let next_for_continuation = next.clone();
    p.add_continuation(Box::new(move |state, result| {
        if let Some(f) = &on_finally {
            let r = apply_function(f, &env, &[], None, pos.clone());
            if r.is_runtime_error() {
                next_for_continuation.reject(r);
                return;
            }
        }
        if state == PromiseState::Rejected {
            next_for_continuation.reject(result);
        } else {
            next_for_continuation.resolve(result);
        }
    }));
    Object::Promise(next)
}

fn resolve_chained_promise(next: &Rc<Promise>, result: Object) {
    match result {
        Object::Promise(promise) => {
            let next = next.clone();
            promise.add_continuation(Box::new(move |state, value| {
                if state == PromiseState::Rejected {
                    next.reject(value);
                } else {
                    next.resolve(value);
                }
            }));
        }
        other if other.is_runtime_error() => next.reject(other),
        other => next.resolve(other),
    }
}
