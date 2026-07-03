//! Property access, class construction, and builtin method dispatch.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{Position, Stmt};
use crate::object::*;

use super::eval_core::eval_block;
use super::expressions::bind_params;
use super::iterator::{default_iterator_method, get_iterator_index, SYMBOL_ITERATOR_KEY};

/// Read a named property of a value, dispatching on the value's type.
pub fn get_property(obj: &Object, name: &str, pos: Position) -> Object {
    match obj {
        Object::Hash(h) => {
            let hb = h.borrow_mut();
            if let Some(v) = hb.get(name) {
                return v.clone();
            }
            if let Some(proto) = hb.proto.clone() {
                drop(hb);
                return get_property(&proto, name, pos);
            }
            if name == SYMBOL_ITERATOR_KEY {
                return default_iterator_method(Object::Hash(h.clone()), "Object.Symbol.iterator");
            }
            if name == "hasOwnProperty" {
                let target = Object::Hash(h.clone());
                return native_builtin(
                    "Object.hasOwnProperty",
                    move |ctx, args| {
                        let Some(key) = args.first() else {
                            return Object::Boolean(false);
                        };
                        let receiver = ctx.receiver.as_ref().unwrap_or(&target);
                        match receiver {
                            Object::Hash(hash) => {
                                Object::Boolean(hash.borrow().contains(&key.inspect()))
                            }
                            _ => Object::Boolean(false),
                        }
                    },
                    Some(Object::Hash(h.clone())),
                );
            }
            Object::Undefined
        }
        Object::Instance(i) => {
            let ib = i.borrow_mut();
            if let Some(v) = ib.props.get(name) {
                return v.clone();
            }
            let class = ib.class.clone();
            drop(ib);
            // Look up the method on the class chain.
            let mut current = Some(class.clone());
            while let Some(c) = current {
                if let Some(m) = c.borrow_mut().methods.get(name).cloned() {
                    return bind_method_value(m, obj.clone());
                }
                if let Some(s) = c.borrow_mut().statics.get(name) {
                    return s.clone();
                }
                current = c.borrow_mut().super_.clone();
            }
            new_error(
                pos,
                format!(
                    "TypeError: '{}' is not a property of {}",
                    name,
                    class.borrow_mut().name
                ),
            )
        }
        Object::Class(c) => {
            if let Some(v) = c.borrow_mut().statics.get(name) {
                return v.clone();
            }
            if let Some(m) = c.borrow_mut().methods.get(name) {
                return m.clone();
            }
            new_error(
                pos,
                format!(
                    "TypeError: '{}' is not a static member of {}",
                    name,
                    c.borrow_mut().name
                ),
            )
        }
        Object::Builtin(b) => {
            if b.name == "Date" && name == "now" {
                return native_builtin(
                    "Date.now",
                    |_, _| {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as f64;
                        Object::Number(now)
                    },
                    None,
                );
            }
            Object::Undefined
        }
        Object::String(s) => {
            if name == "length" {
                return Object::Number(s.chars().count() as f64);
            }
            if name == SYMBOL_ITERATOR_KEY {
                return default_iterator_method(obj.clone(), "String.Symbol.iterator");
            }
            if let Some(f) = string_method(name) {
                return Object::Builtin(Rc::new(Builtin {
                    name: format!("String.{}", name),
                    func: f,
                    extra: Some(obj.clone()),
                }));
            }
            new_error(
                pos,
                format!("TypeError: cannot read property '{}' of string", name),
            )
        }
        Object::Number(_) => {
            if let Some(f) = number_method(name) {
                return Object::Builtin(Rc::new(Builtin {
                    name: format!("Number.{}", name),
                    func: f,
                    extra: Some(obj.clone()),
                }));
            }
            Object::Undefined
        }
        Object::Array(a) => {
            if name == "length" {
                return Object::Number(a.borrow_mut().elements.len() as f64);
            }
            if name == SYMBOL_ITERATOR_KEY {
                return default_iterator_method(obj.clone(), "Array.Symbol.iterator");
            }
            if let Some(f) = array_method(name) {
                return Object::Builtin(Rc::new(Builtin {
                    name: format!("Array.{}", name),
                    func: f,
                    extra: Some(obj.clone()),
                }));
            }
            new_error(
                pos,
                format!("TypeError: '{}' is not a function of array", name),
            )
        }
        Object::Error(e) => {
            let eb = e.borrow_mut();
            match name {
                "name" => str_obj(if eb.name.is_empty() {
                    "Error".into()
                } else {
                    eb.name.clone()
                }),
                "message" => str_obj(eb.message.clone()),
                "stack" => str_obj(eb.stack.clone()),
                _ => Object::Undefined,
            }
        }
        Object::Boolean(b) => {
            if name == "toString" {
                let value = *b;
                return native_builtin(
                    "Boolean.toString",
                    move |_, _| str_obj(value.to_string()),
                    None,
                );
            }
            Object::Undefined
        }
        Object::Null => Object::Undefined,
        Object::Undefined => Object::Undefined,
        Object::Promise(p) => {
            if let Some(f) = promise_method(name) {
                return Object::Builtin(Rc::new(Builtin {
                    name: format!("Promise.{}", name),
                    func: f,
                    extra: Some(Object::Promise(p.clone())),
                }));
            }
            Object::Undefined
        }
        Object::Date(ms) => {
            if let Some(f) = date_method(name) {
                return Object::Builtin(Rc::new(Builtin {
                    name: format!("Date.{}", name),
                    func: f,
                    extra: Some(Object::Date(*ms)),
                }));
            }
            Object::Undefined
        }
        Object::Regexp(r) => match name {
            "source" => str_obj(r.source.clone()),
            "flags" => str_obj(r.flags.clone()),
            _ => {
                if let Some(f) = regexp_method(name) {
                    return Object::Builtin(Rc::new(Builtin {
                        name: format!("RegExp.{}", name),
                        func: f,
                        extra: Some(obj.clone()),
                    }));
                }
                Object::Undefined
            }
        },
        Object::Map(m) => {
            if name == "size" {
                return Object::Number(m.borrow().size() as f64);
            }
            if name == SYMBOL_ITERATOR_KEY {
                return default_iterator_method(obj.clone(), "Map.Symbol.iterator");
            }
            if let Some(f) = map_method(name) {
                return Object::Builtin(Rc::new(Builtin {
                    name: format!("Map.{}", name),
                    func: f,
                    extra: Some(obj.clone()),
                }));
            }
            Object::Undefined
        }
        Object::Set(s) => {
            if name == "size" {
                return Object::Number(s.borrow().size() as f64);
            }
            if name == SYMBOL_ITERATOR_KEY {
                return default_iterator_method(obj.clone(), "Set.Symbol.iterator");
            }
            if let Some(f) = set_method(name) {
                return Object::Builtin(Rc::new(Builtin {
                    name: format!("Set.{}", name),
                    func: f,
                    extra: Some(obj.clone()),
                }));
            }
            Object::Undefined
        }
        _ => new_error(
            pos,
            format!(
                "TypeError: cannot read property '{}' of {}",
                name,
                obj.type_tag()
            ),
        ),
    }
}

/// Read an indexed value (arr[i], obj[key], str[i]).
pub fn get_index(obj: &Object, key: &Object, pos: Position) -> Object {
    if let Some(iterator) = get_iterator_index(obj, key, pos.clone()) {
        return iterator;
    }
    match obj {
        Object::Array(a) => {
            if let Object::Number(n) = key {
                let i = *n as isize;
                let arr = a.borrow_mut();
                let len = arr.elements.len() as isize;
                if i >= 0 && i < len {
                    return arr.elements[i as usize].clone();
                }
            }
            Object::Undefined
        }
        Object::Hash(h) => {
            let k = key.inspect();
            if let Some(v) = h.borrow_mut().get(&k) {
                return v.clone();
            }
            if let Some(proto) = &h.borrow_mut().proto.clone() {
                return get_index(proto, key, pos);
            }
            Object::Undefined
        }
        Object::String(s) => {
            if let Object::Number(n) = key {
                let i = *n as usize;
                if let Some(c) = s.chars().nth(i) {
                    return str_obj(c.to_string());
                }
            }
            Object::Undefined
        }
        Object::Instance(i) => {
            if let Object::String(k) = key {
                if let Some(v) = i.borrow_mut().props.get(k.as_str()).cloned() {
                    return v;
                }
            }
            Object::Undefined
        }
        _ => new_error(pos, format!("TypeError: cannot index {}", obj.type_tag())),
    }
}

/// Bind a method function to `this`, returning a fresh closure with `this` set.
fn bind_method(method: &Rc<Function>, this: Object) -> Object {
    let bound = Rc::new(Function {
        name: method.name.clone(),
        params: method.params.clone(),
        body: method.body.clone(),
        env: method.env.clone(),
        is_async: method.is_async,
        return_t: method.return_t.clone(),
        pos: method.pos.clone(),
        lexical_this: method.lexical_this,
    });
    bound.env.borrow_mut().this = Some(this);
    Object::Function(bound)
}

fn bind_method_value(method: Object, this: Object) -> Object {
    match method {
        Object::Function(f) => bind_method(&f, this),
        Object::Closure(c) => {
            c.home_env.borrow_mut().this = Some(this);
            Object::Closure(c)
        }
        other => other,
    }
}

/// Construct an instance of a class.
pub fn construct_class(
    cls: &Rc<RefCell<Class>>,
    env: &EnvRef,
    args: &[Object],
    pos: Position,
) -> Object {
    let inst = Rc::new(RefCell::new(Instance {
        class: cls.clone(),
        props: HashMap::new(),
        pos: pos.clone(),
    }));
    // Initialize fields from the class chain.
    let mut current = Some(cls.clone());
    while let Some(c) = current {
        for (k, v) in c.borrow_mut().fields.iter() {
            inst.borrow_mut().props.insert(k.clone(), v.clone());
        }
        current = c.borrow_mut().super_.clone();
    }
    // Native constructor (Error, etc.).
    if let Some(native) = cls.borrow_mut().native_ctor.clone() {
        let mut ctx = CallContext::new(env, pos.clone());
        let _ = native(&mut ctx, &inst, args);
        return Object::Instance(inst);
    }
    // Implicit super() if the derived constructor doesn't call it.
    let super_ = cls.borrow_mut().super_.clone();
    let ctor = cls.borrow_mut().methods.get("constructor").cloned();
    let needs_implicit_super = super_.is_some()
        && match &ctor {
            Some(Object::Function(f)) => !constructor_calls_super(&f.body),
            Some(Object::Closure(c)) => !block_calls_super(&c.proto.body),
            None => true,
            _ => false,
        };
    if needs_implicit_super {
        if let Some(sc) = &super_ {
            let r = call_constructor(sc, &inst, env, args, pos.clone());
            if r.is_runtime_error() {
                return r;
            }
        }
    }
    // Run the constructor (bound to `this`).
    if let Some(con) = &ctor {
        let r = call_constructor_value(con, cls, &inst, env, args, pos.clone());
        if r.is_runtime_error() {
            return r;
        }
    }
    Object::Instance(inst)
}

/// Call a class constructor (for super()).
fn call_constructor(
    cls: &Rc<RefCell<Class>>,
    inst: &Rc<RefCell<Instance>>,
    env: &EnvRef,
    args: &[Object],
    pos: Position,
) -> Object {
    if let Some(native) = cls.borrow_mut().native_ctor.clone() {
        let mut ctx = CallContext::new(env, pos);
        return native(&mut ctx, inst, args);
    }
    let ctor = match cls.borrow_mut().methods.get("constructor").cloned() {
        Some(ctor) => ctor,
        _ => return Object::Undefined,
    };
    call_constructor_value(&ctor, cls, inst, env, args, pos)
}

/// super(...) constructor invocation.
pub fn call_super_constructor(env: &EnvRef, args: &[Object], pos: Position) -> Object {
    let this = env.borrow_mut().this.clone();
    let inst = match &this {
        Some(Object::Instance(i)) => i.clone(),
        _ => return new_error(pos, "ReferenceError: super is not available"),
    };
    let current = env
        .borrow_mut()
        .constructor_class
        .clone()
        .unwrap_or_else(|| inst.borrow_mut().class.clone());
    let super_ = match current.borrow_mut().super_.clone() {
        Some(s) => s,
        None => return new_error(pos, "ReferenceError: super is not available"),
    };
    call_constructor(&super_, &inst, env, args, pos)
}

pub fn get_super_constructor(env: &EnvRef, pos: Position) -> Object {
    let this = env.borrow_mut().this.clone();
    let inst = match &this {
        Some(Object::Instance(i)) => i.clone(),
        _ => return new_error(pos, "ReferenceError: super is not available"),
    };
    let current = env
        .borrow_mut()
        .constructor_class
        .clone()
        .unwrap_or_else(|| inst.borrow_mut().class.clone());
    let super_ = match current.borrow_mut().super_.clone() {
        Some(s) => s,
        None => return new_error(pos, "ReferenceError: super is not available"),
    };
    let ctor = super_.borrow_mut().methods.get("constructor").cloned();
    match ctor {
        Some(ctor) => bind_constructor_value(ctor, super_, Object::Instance(inst)),
        None => Object::Undefined,
    }
}

/// Get a super method (super.method).
pub fn get_super_method(env: &EnvRef, name: &str, pos: Position) -> Object {
    let this = env.borrow_mut().this.clone();
    let inst = match &this {
        Some(Object::Instance(i)) => i.clone(),
        _ => return new_error(pos, "ReferenceError: super is not available"),
    };
    let super_ = match inst.borrow_mut().class.borrow_mut().super_.clone() {
        Some(s) => s,
        None => return new_error(pos, "ReferenceError: super is not available"),
    };
    if let Some(m) = super_.borrow_mut().methods.get(name).cloned() {
        return bind_method_value(m, Object::Instance(inst));
    }
    new_error(pos, format!("TypeError: super.{} is not a method", name))
}

fn bind_constructor_value(method: Object, class: Rc<RefCell<Class>>, this: Object) -> Object {
    match method {
        Object::Function(f) => {
            let bound = bind_method(&f, this);
            if let Object::Function(func) = &bound {
                func.env.borrow_mut().constructor_class = Some(class);
            }
            bound
        }
        Object::Closure(c) => {
            {
                let mut env = c.home_env.borrow_mut();
                env.this = Some(this);
                env.constructor_class = Some(class);
            }
            Object::Closure(c)
        }
        other => other,
    }
}

fn call_constructor_value(
    ctor: &Object,
    cls: &Rc<RefCell<Class>>,
    inst: &Rc<RefCell<Instance>>,
    env: &EnvRef,
    args: &[Object],
    pos: Position,
) -> Object {
    match ctor {
        Object::Function(con) => {
            let scope = Environment::child(&con.env);
            scope.borrow_mut().this = Some(Object::Instance(inst.clone()));
            scope.borrow_mut().constructor_class = Some(cls.clone());
            if let Err(e) = bind_params(&scope, env, &con.params, args, pos) {
                return e;
            }
            let r = eval_block(&con.body, &scope);
            if let Object::Return(ret) = r {
                *ret
            } else {
                r
            }
        }
        Object::Closure(c) => {
            {
                let mut home = c.home_env.borrow_mut();
                home.this = Some(Object::Instance(inst.clone()));
                home.constructor_class = Some(cls.clone());
            }
            match crate::bytecode::call::call_closure_with_this(
                c,
                args,
                env,
                Some(Object::Instance(inst.clone())),
                pos,
            ) {
                Ok(v) => v,
                Err(e) => e,
            }
        }
        _ => Object::Undefined,
    }
}

/// Construct a value from a builtin constructor (new Date(...), new Map(...), etc.).
pub fn construct_builtin(b: &Builtin, env: &EnvRef, args: &[Object], pos: Position) -> Object {
    let _name = b.name.clone();
    let mut ctx = CallContext::new(env, pos);
    (b.func)(&mut ctx, args)
}

/// Run an async function: schedule the body on a thread, resolving/rejecting
/// a new promise with the result.
pub fn run_async_function(
    f: &Rc<Function>,
    scope: &EnvRef,
    caller: &EnvRef,
    args: &[Object],
    pos: Position,
) -> Object {
    let promise = Promise::new();
    if let Err(e) = bind_params(scope, caller, &f.params, args, pos) {
        promise.reject(e);
        return Object::Promise(promise);
    }
    // Single-threaded model: run the async body inline on the calling thread.
    // `await` within it blocks via Promise::wait(), which is acceptable for this
    // interpreter.
    let result = eval_block(&f.body, scope);
    match result {
        Object::Return(r) => promise.resolve(*r),
        Object::Error(e) => promise.reject(Object::Error(e)),
        other => {
            if other.is_runtime_error() {
                promise.reject(other);
            } else {
                promise.resolve(other);
            }
        }
    }
    Object::Promise(promise)
}

/// Whether a constructor body contains a `super(...)` call (naive scan).
fn constructor_calls_super(body: &crate::ast::BlockStmt) -> bool {
    block_calls_super(body)
}

fn block_calls_super(body: &crate::ast::BlockStmt) -> bool {
    body.statements.iter().any(node_calls_super_stmt)
}

fn node_calls_super_stmt(s: &crate::ast::Stmt) -> bool {
    match s {
        Stmt::Expr(e) => node_calls_super_expr(&e.expr),
        Stmt::Block(b) => block_calls_super(b),
        Stmt::If(i) => {
            block_calls_super(&i.consequence)
                || i.alternative
                    .as_deref()
                    .map(node_calls_super_stmt)
                    .unwrap_or(false)
        }
        _ => false,
    }
}

fn node_calls_super_expr(e: &crate::ast::Expr) -> bool {
    match e {
        crate::ast::Expr::Call(c) => {
            matches!(&c.callee, crate::ast::Expr::Super(_)) || node_calls_super_expr(&c.callee)
        }
        _ => false,
    }
}

/// Build a simple native builtin with no captured receiver.
pub fn native_builtin(
    name: &str,
    func: impl Fn(&mut CallContext<'_>, &[Object]) -> Object + 'static,
    extra: Option<Object>,
) -> Object {
    Object::Builtin(Rc::new(Builtin {
        name: name.into(),
        func: Rc::new(func),
        extra,
    }))
}

// ============================================================================
// Method tables — populated in methods_impl.rs
// ============================================================================

use super::builtins;

pub fn array_method(name: &str) -> Option<BuiltinFn> {
    builtins::array_method(name)
}
pub fn string_method(name: &str) -> Option<BuiltinFn> {
    builtins::string_method(name)
}
pub fn number_method(name: &str) -> Option<BuiltinFn> {
    builtins::number_method(name)
}
pub fn promise_method(name: &str) -> Option<BuiltinFn> {
    builtins::promise_method(name)
}
pub fn regexp_method(name: &str) -> Option<BuiltinFn> {
    builtins::regexp_method(name)
}
pub fn map_method(name: &str) -> Option<BuiltinFn> {
    builtins::map_method(name)
}
pub fn set_method(name: &str) -> Option<BuiltinFn> {
    builtins::set_method(name)
}
pub fn date_method(name: &str) -> Option<BuiltinFn> {
    builtins::date_method(name)
}
