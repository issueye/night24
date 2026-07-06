//! Global builtins (Math, JSON, Object, Array, etc.) and the method tables.

use std::cell::RefCell;
use std::rc::Rc;

use crate::object::*;

use super::console::console_object;
use super::expressions::apply_function;

mod array;
mod collections;
mod date;
mod globals;
mod json;
mod math;
mod object;
mod primitive;
mod promise;
mod string;

use array::array_global;
pub use array::array_method;
use collections::{map_global, set_global};
pub use collections::{map_method, set_method};
pub use date::date_method;
use globals::{builtin_is_finite, builtin_is_nan, builtin_parse_float, builtin_parse_int};
use json::json_object;
use math::math_object;
use object::object_global;
pub use primitive::number_method;
use primitive::{boolean_global, number_global};
pub use promise::promise_method;
use promise::{attach_promise_statics, promise_constructor};
use string::string_global;
pub use string::string_method;

type FnPtr = BuiltinFn;

/// Register all standard globals on the VM.
pub fn register_globals(vm: &Rc<VirtualMachine>) {
    vm.set_global("console", console_object());

    let println_fn: FnPtr = Rc::new(|ctx, args| {
        let parts: Vec<String> = args.iter().map(|a| a.inspect()).collect();
        ctx.vm().push_stdout(parts.join(""));
        Object::Undefined
    });
    vm.set_global(
        "println",
        Object::Builtin(Rc::new(Builtin {
            name: "println".into(),
            func: println_fn,
            extra: None,
        })),
    );

    let print_fn: FnPtr = Rc::new(|ctx, args| {
        let text = args
            .iter()
            .map(|a| a.inspect())
            .collect::<Vec<_>>()
            .join("");
        ctx.vm().push_stdout(text);
        Object::Undefined
    });
    vm.set_global(
        "print",
        Object::Builtin(Rc::new(Builtin {
            name: "print".into(),
            func: print_fn,
            extra: None,
        })),
    );

    vm.set_global("Math", math_object());
    vm.set_global("JSON", json_object());
    vm.set_global("Object", object_global());
    vm.set_global("Array", array_global());
    vm.set_global("String", string_global());
    vm.set_global("Number", number_global());
    vm.set_global("Boolean", boolean_global());
    vm.set_global("Symbol", super::iterator::symbol_global());

    // Error constructors.
    for name in [
        "Error",
        "TypeError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
    ] {
        let n = name.to_string();
        let f: FnPtr = Rc::new(move |_ctx, args| {
            let message = args.first().map(|a| a.inspect()).unwrap_or_default();
            new_error_object(crate::ast::Position::default(), &n, message)
        });
        vm.set_global(
            name,
            Object::Builtin(Rc::new(Builtin {
                name: name.into(),
                func: f,
                extra: None,
            })),
        );
    }

    // Promise constructor.
    let promise_fn: FnPtr = Rc::new(promise_constructor);
    vm.set_global(
        "Promise",
        Object::Builtin(Rc::new(Builtin {
            name: "Promise".into(),
            func: promise_fn,
            extra: None,
        })),
    );

    // Date: callable returning the current epoch millis.
    let date_fn: FnPtr = Rc::new(|_ctx, _args| Object::Date(chrono_now_millis()));
    vm.set_global(
        "Date",
        Object::Builtin(Rc::new(Builtin {
            name: "Date".into(),
            func: date_fn,
            extra: None,
        })),
    );

    // setTimeout / clearTimeout / setInterval / sleepAsync.
    register_timers(vm);
    register_collections_and_date(vm);

    // Conversion functions.
    vm.set_global("parseInt", native("parseInt", builtin_parse_int));
    vm.set_global("parseFloat", native("parseFloat", builtin_parse_float));
    vm.set_global("isNaN", native("isNaN", builtin_is_nan));
    vm.set_global("isFinite", native("isFinite", builtin_is_finite));
    vm.set_global("String", string_global());
    vm.set_global("Number", number_global());
}

fn native(
    name: &str,
    func: impl Fn(&mut CallContext<'_>, &[Object]) -> Object + 'static,
) -> Object {
    Object::Builtin(Rc::new(Builtin {
        name: name.into(),
        func: Rc::new(func),
        extra: None,
    }))
}

fn as_num(o: Option<&Object>) -> f64 {
    match o {
        Some(Object::Number(n)) => *n,
        _ => 0.0,
    }
}

fn chrono_now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ============================================================================
// Timers
// ============================================================================

fn register_timers(vm: &Rc<VirtualMachine>) {
    let vm_clone = vm.clone();
    let set_timeout: FnPtr = Rc::new(move |ctx, args| {
        let callback = args.first().cloned().unwrap_or(Object::Undefined);
        let ms = as_num(args.get(1)) as u64;
        let env = ctx.env.clone();
        let id = vm_clone.next_timer_id();
        vm_clone.async_add(1);
        let vm = vm_clone.clone();
        // Single-threaded model: run the callback inline after sleeping so a
        // synchronous top-level script observes it before the process exits.
        if ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        let _ = apply_function(&callback, &env, &[], None, crate::ast::Position::default());
        vm.async_done();
        Object::Number(id as f64)
    });
    vm.set_global(
        "setTimeout",
        Object::Builtin(Rc::new(Builtin {
            name: "setTimeout".into(),
            func: set_timeout,
            extra: None,
        })),
    );

    let sleep_async: FnPtr = Rc::new(|_ctx, args| {
        let ms = as_num(args.first()) as u64;
        let promise = Promise::new();
        if ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        promise.resolve(Object::Undefined);
        Object::Promise(promise)
    });
    vm.set_global(
        "sleepAsync",
        Object::Builtin(Rc::new(Builtin {
            name: "sleepAsync".into(),
            func: sleep_async,
            extra: None,
        })),
    );

    let vm_clone = vm.clone();
    let set_interval: FnPtr = Rc::new(move |ctx, args| {
        let callback = args.first().cloned().unwrap_or(Object::Undefined);
        let ms = as_num(args.get(1)) as u64;
        let env = ctx.env.clone();
        let vm = vm_clone.clone();
        let id = vm.next_timer_id();
        vm.async_add(1);
        // Run a bounded number of times to avoid hanging the process.
        let mut count = 0u32;
        while count < 1000 {
            std::thread::sleep(std::time::Duration::from_millis(ms.max(1)));
            let _ = apply_function(&callback, &env, &[], None, crate::ast::Position::default());
            count += 1;
        }
        vm.async_done();
        Object::Number(id as f64)
    });
    vm.set_global(
        "setInterval",
        Object::Builtin(Rc::new(Builtin {
            name: "setInterval".into(),
            func: set_interval,
            extra: None,
        })),
    );

    attach_promise_statics(vm);
}

fn register_collections_and_date(vm: &Rc<VirtualMachine>) {
    vm.set_global("Map", map_global());
    vm.set_global("Set", set_global());

    // Date constructor
    let date_constructor: FnPtr = Rc::new(|_ctx, args| {
        if args.is_empty() {
            // new Date() - current time
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            return Object::Date(now);
        }
        // new Date(milliseconds)
        if let Some(Object::Number(n)) = args.first() {
            return Object::Date(*n as i64);
        }
        // new Date(year, month, day, ...)
        if args.len() >= 2 {
            let year = match args.first() {
                Some(Object::Number(n)) => *n as i32,
                _ => 1970,
            };
            let month = match args.get(1) {
                Some(Object::Number(n)) => (*n as u32 + 1).clamp(1, 12), // JS months are 0-indexed
                _ => 1,
            };
            let day = match args.get(2) {
                Some(Object::Number(n)) => (*n as u32).clamp(1, 31),
                _ => 1,
            };
            let hour = match args.get(3) {
                Some(Object::Number(n)) => (*n as u32).min(23),
                _ => 0,
            };
            let minute = match args.get(4) {
                Some(Object::Number(n)) => (*n as u32).min(59),
                _ => 0,
            };
            let second = match args.get(5) {
                Some(Object::Number(n)) => (*n as u32).min(59),
                _ => 0,
            };
            let millisecond = match args.get(6) {
                Some(Object::Number(n)) => (*n as u32).min(999),
                _ => 0,
            };

            // Convert to milliseconds since epoch
            let ms = crate::stdlib::ms_from_utc_parts(
                year,
                month,
                day,
                hour,
                minute,
                second,
                millisecond,
            );
            return Object::Date(ms);
        }
        Object::Date(0)
    });
    vm.set_global(
        "Date",
        Object::Builtin(Rc::new(Builtin {
            name: "Date".into(),
            func: date_constructor,
            extra: None,
        })),
    );
}

// ============================================================================
// Method tables
// ============================================================================

fn active_regexp(ctx: &CallContext) -> Option<Rc<RegexpData>> {
    match &ctx.receiver {
        Some(Object::Regexp(r)) => Some(r.clone()),
        _ => None,
    }
}

fn normalize_index(idx: isize, len: isize) -> isize {
    if idx < 0 {
        (len + idx).max(0)
    } else {
        idx
    }
}

// ============================================================================
// Promise / RegExp methods
// ============================================================================

pub fn regexp_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "test" => Some(rex_test),
        "exec" => Some(rex_exec),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}
fn rex_test(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(r) = active_regexp(ctx) {
        if let Some(Object::String(s)) = args.first() {
            return Object::Boolean(r.re.is_match(s));
        }
    }
    Object::Boolean(false)
}
fn rex_exec(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(r) = active_regexp(ctx) {
        if let Some(Object::String(s)) = args.first() {
            if let Some(m) = r.re.find(s) {
                let elems = vec![str_obj(m.as_str().to_string())];
                return Object::Array(Rc::new(RefCell::new(ArrayData { elements: elems })));
            }
            return Object::Null;
        }
    }
    Object::Null
}
