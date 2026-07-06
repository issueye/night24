use super::super::helpers::*;
use crate::object::{new_error, CallContext, Object};

pub(crate) fn timers_module() -> Object {
    module(vec![
        (
            "setTimeout",
            native("timers.setTimeout", timers_set_timeout),
        ),
        ("clearTimeout", native("timers.clearTimeout", timers_clear)),
        (
            "setInterval",
            native("timers.setInterval", timers_set_interval),
        ),
        (
            "clearInterval",
            native("timers.clearInterval", timers_clear),
        ),
        (
            "queueMicrotask",
            native("timers.queueMicrotask", timers_queue_microtask),
        ),
        ("sleep", native("timers.sleep", timers_sleep)),
        (
            "sleepAsync",
            native("timers.sleepAsync", timers_sleep_async),
        ),
    ])
}

/// Invoke a global builtin by name with the given arguments, returning its
/// result; if the global is absent or not callable, return an Error.
fn forward_global(ctx: &mut CallContext, name: &str, args: &[Object]) -> Object {
    match ctx.vm().get_global(name) {
        Some(Object::Builtin(b)) => {
            let func = b.func.clone();
            let mut inner_ctx = CallContext::new(ctx.env, ctx.pos.clone());
            func(&mut inner_ctx, args)
        }
        Some(_) => new_error(
            ctx.pos.clone(),
            format!("timers.{}: global {} is not a builtin", name, name),
        ),
        None => new_error(
            ctx.pos.clone(),
            format!("timers.{}: global builtin {} not found", name, name),
        ),
    }
}

pub(crate) fn timers_set_timeout(ctx: &mut CallContext, args: &[Object]) -> Object {
    forward_global(ctx, "setTimeout", args)
}

pub(crate) fn timers_set_interval(ctx: &mut CallContext, args: &[Object]) -> Object {
    forward_global(ctx, "setInterval", args)
}

pub(crate) fn timers_sleep_async(ctx: &mut CallContext, args: &[Object]) -> Object {
    forward_global(ctx, "sleepAsync", args)
}

/// Synchronous sleep: blocks the calling thread for `ms` milliseconds.
fn timers_sleep(_ctx: &mut CallContext, args: &[Object]) -> Object {
    match args.first() {
        Some(Object::Number(n)) if *n > 0.0 => {
            std::thread::sleep(std::time::Duration::from_millis(*n as u64));
        }
        _ => {}
    }
    Object::Undefined
}

/// In a synchronous single-threaded runtime, clear* are no-ops: by the time
/// the script observes a timer id the callback has already executed inline.
fn timers_clear(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    Object::Undefined
}

/// queueMicrotask runs the callback immediately on the current thread.
fn timers_queue_microtask(ctx: &mut CallContext, args: &[Object]) -> Object {
    let callback = match args.first() {
        Some(value) if is_callable(value) => value.clone(),
        _ => return Object::Undefined,
    };
    let _ = crate::evaluator::expressions::apply_function(
        &callback,
        ctx.env,
        &[],
        None,
        ctx.pos.clone(),
    );
    Object::Undefined
}

// ---------------------------------------------------------------------------
// glob: thin deterministic wrapper over the existing fs glob engine.
// ---------------------------------------------------------------------------
