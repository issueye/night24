use super::super::helpers::*;
use super::net_http_client::{http_client_get, http_client_post, http_client_request};
use crate::object::{new_error, CallContext, Object};

pub(crate) fn async_module() -> Object {
    module(vec![
        ("fetchAsync", native("async.fetchAsync", async_fetch)),
        ("getAsync", native("async.getAsync", async_get)),
        ("postAsync", native("async.postAsync", async_post)),
        ("runWorker", native("async.runWorker", async_run_worker)),
    ])
}

fn resolved_promise(value: Object) -> Object {
    let promise = crate::object::Promise::new();
    promise.resolve(value);
    Object::Promise(promise)
}

fn rejected_promise(reason: Object) -> Object {
    let promise = crate::object::Promise::new();
    promise.reject(reason);
    Object::Promise(promise)
}

fn async_get(ctx: &mut CallContext, args: &[Object]) -> Object {
    let result = http_client_get(ctx, args);
    match result.is_runtime_error() {
        true => rejected_promise(result),
        false => resolved_promise(result),
    }
}

fn async_post(ctx: &mut CallContext, args: &[Object]) -> Object {
    let result = http_client_post(ctx, args);
    match result.is_runtime_error() {
        true => rejected_promise(result),
        false => resolved_promise(result),
    }
}

fn async_fetch(ctx: &mut CallContext, args: &[Object]) -> Object {
    let result = http_client_request(ctx, args);
    match result.is_runtime_error() {
        true => rejected_promise(result),
        false => resolved_promise(result),
    }
}

fn async_run_worker(ctx: &mut CallContext, args: &[Object]) -> Object {
    let func = match args.first() {
        Some(value) if is_callable(value) => value.clone(),
        _ => {
            return rejected_promise(new_error(
                ctx.pos.clone(),
                "async.runWorker: first argument must be a function",
            ))
        }
    };
    let worker_args: Vec<Object> = args.iter().skip(1).cloned().collect();
    let result = call_script_function(&func, ctx.env, &worker_args);
    match result.is_runtime_error() {
        true => rejected_promise(result),
        false => resolved_promise(result),
    }
}
