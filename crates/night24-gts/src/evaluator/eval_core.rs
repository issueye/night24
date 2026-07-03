//! Core evaluator: dispatch and statement evaluation.

use std::cell::RefCell;
use std::rc::Rc;

use crate::ast::*;
use crate::object::*;

/// Control-flow signals encoded as thrown errors with sentinel messages.
pub const BREAK_SIGNAL: &str = "__break__";
pub const CONTINUE_SIGNAL: &str = "__continue__";

/// Evaluate a program AST under the given environment.
pub fn eval_program(prog: &Program, env: &EnvRef) -> Object {
    let mut result = Object::Undefined;
    for stmt in &prog.body {
        if let Some(timeout) = env.borrow().vm.check_timeout(stmt.pos()) {
            return timeout;
        }
        result = eval_stmt(stmt, env);
        if let Object::Return(r) = &result {
            if let Object::Error(e) = r.as_ref() {
                let name = e.borrow_mut().name.clone();
                let msg = e.borrow_mut().message.clone();
                if msg == BREAK_SIGNAL {
                    return new_error(
                        e.borrow_mut().pos.clone(),
                        "SyntaxError: break outside loop",
                    );
                }
                if msg == CONTINUE_SIGNAL {
                    return new_error(
                        e.borrow_mut().pos.clone(),
                        "SyntaxError: continue outside loop",
                    );
                }
                let _ = name;
            }
            return (**r).clone();
        }
        if result.is_runtime_error() {
            return result;
        }
    }
    result
}

/// Evaluate any statement.
pub fn eval_stmt(stmt: &Stmt, env: &EnvRef) -> Object {
    if let Some(timeout) = env.borrow().vm.check_timeout(stmt.pos()) {
        return timeout;
    }
    match stmt {
        Stmt::Let(s) => eval_let(s, env),
        Stmt::Const(s) => eval_const(s, env),
        Stmt::Var(s) => eval_var(s, env),
        Stmt::Block(b) => {
            let scope = Environment::child(env);
            eval_block(b, &scope)
        }
        Stmt::If(s) => eval_if(s, env),
        Stmt::While(s) => eval_while(s, env),
        Stmt::For(s) => eval_for(s, env),
        Stmt::ForIn(s) => eval_for_in(s, env),
        Stmt::ForOf(s) => eval_for_of(s, env),
        Stmt::Return(s) => eval_return(s, env),
        Stmt::Break(s) => Object::Return(Box::new(new_error(
            s.pos.clone(),
            format!("SyntaxError: {}", BREAK_SIGNAL),
        ))),
        Stmt::Continue(s) => Object::Return(Box::new(new_error(
            s.pos.clone(),
            format!("SyntaxError: {}", CONTINUE_SIGNAL),
        ))),
        Stmt::Throw(s) => eval_throw(s, env),
        Stmt::Try(s) => eval_try(s, env),
        Stmt::Expr(s) => crate::evaluator::expressions::eval_expr(&s.expr, env),
        Stmt::Labeled(s) => eval_stmt(&s.stmt, env),
        Stmt::FuncDecl(s) => eval_func_decl(s, env),
        Stmt::ClassDecl(s) => eval_class_decl(s, env),
        Stmt::Import(s) => eval_import(s, env),
        Stmt::Export(s) => eval_export(s, env),
    }
}

/// Dispatch helper used by the VM callback path.
pub fn eval_node(node: NodeRef, env: &EnvRef, _vm: &Rc<VirtualMachine>) -> Object {
    match node {
        NodeRef::Program(p) => eval_program(&p, env),
        NodeRef::Stmt(s) => eval_stmt(&s, env),
        NodeRef::Expr(e) => crate::evaluator::expressions::eval_expr(&e, env),
    }
}

/// A thin evaluator handle retained for future type-check / config knobs.
pub struct Eval;

fn eval_let(s: &LetStmt, env: &EnvRef) -> Object {
    if let Some(binding) = &s.binding {
        return eval_destructure(binding, s.value.as_ref(), s.type_anno.as_ref(), false, env);
    }
    let mut val = Object::Undefined;
    if let Some(v) = &s.value {
        val = crate::evaluator::expressions::eval_expr(v, env);
        if val.is_runtime_error() {
            return val;
        }
    }
    env.borrow_mut()
        .set_typed(s.name.clone(), val.clone(), s.type_anno.clone());
    Object::Undefined
}

fn eval_const(s: &ConstStmt, env: &EnvRef) -> Object {
    if let Some(binding) = &s.binding {
        return eval_destructure(binding, s.value.as_ref(), s.type_anno.as_ref(), true, env);
    }
    let mut val = Object::Undefined;
    if let Some(v) = &s.value {
        val = crate::evaluator::expressions::eval_expr(v, env);
        if val.is_runtime_error() {
            return val;
        }
    }
    env.borrow_mut()
        .set_typed_const(s.name.clone(), val, s.type_anno.clone());
    Object::Undefined
}

fn eval_var(s: &VarStmt, env: &EnvRef) -> Object {
    if let Some(binding) = &s.binding {
        return eval_destructure(binding, s.value.as_ref(), s.type_anno.as_ref(), false, env);
    }
    let mut val = Object::Undefined;
    if let Some(v) = &s.value {
        val = crate::evaluator::expressions::eval_expr(v, env);
        if val.is_runtime_error() {
            return val;
        }
    }
    env.borrow_mut()
        .set_typed(s.name.clone(), val.clone(), s.type_anno.clone());
    Object::Undefined
}

/// Destructure a value into a binding pattern (B3.2). `value` is the source
/// expression (must be present for destructuring). `is_const` binds the slots
/// as constants. Element defaults apply when the source slot is `undefined`.
fn eval_destructure(
    binding: &crate::ast::BindingPattern,
    value: Option<&crate::ast::Expr>,
    type_anno: Option<&crate::ast::TypeAnnotation>,
    is_const: bool,
    env: &EnvRef,
) -> Object {
    let src = match value {
        Some(v) => crate::evaluator::expressions::eval_expr(v, env),
        None => Object::Undefined,
    };
    if src.is_runtime_error() {
        return src;
    }
    let pos = crate::ast::Position::default();
    let bind_one = |name: &str, val: Object| {
        if is_const {
            env.borrow_mut()
                .set_typed_const(name.to_string(), val, type_anno.cloned());
        } else {
            env.borrow_mut()
                .set_typed(name.to_string(), val, type_anno.cloned());
        }
    };
    match binding {
        crate::ast::BindingPattern::Array(elems) => {
            for (i, elem) in elems.iter().enumerate() {
                if elem.is_rest {
                    // Collect the tail from index i onward into an array.
                    let tail = if let Object::Array(arr) = &src {
                        let arr = arr.borrow();
                        arr.elements[i.min(arr.elements.len())..].to_vec()
                    } else {
                        Vec::new()
                    };
                    bind_one(
                        &elem.name,
                        Object::Array(std::rc::Rc::new(std::cell::RefCell::new(
                            crate::object::ArrayData { elements: tail },
                        ))),
                    );
                    break;
                }
                if elem.name.is_empty() {
                    continue; // hole
                }
                let mut val = crate::evaluator::methods::get_index(
                    &src,
                    &crate::object::num_obj(i as f64),
                    pos.clone(),
                );
                if matches!(val, Object::Undefined) {
                    if let Some(def) = &elem.default {
                        val = crate::evaluator::expressions::eval_expr(def, env);
                    }
                }
                bind_one(&elem.name, val);
            }
        }
        crate::ast::BindingPattern::Object(elems) => {
            for elem in elems {
                let mut val = crate::evaluator::methods::get_property(&src, &elem.key, pos.clone());
                if matches!(val, Object::Undefined) {
                    if let Some(def) = &elem.default {
                        val = crate::evaluator::expressions::eval_expr(def, env);
                    }
                }
                bind_one(&elem.target, val);
            }
        }
    }
    Object::Undefined
}

pub fn eval_block(block: &BlockStmt, env: &EnvRef) -> Object {
    let mut result = Object::Undefined;
    for stmt in &block.statements {
        if let Some(timeout) = env.borrow().vm.check_timeout(stmt.pos()) {
            return timeout;
        }
        result = eval_stmt(stmt, env);
        if matches!(result, Object::Return(_)) {
            return result;
        }
        if result.is_runtime_error() && control_signal(&result).is_none() {
            return result;
        }
    }
    result
}

fn eval_if(s: &IfStmt, env: &EnvRef) -> Object {
    let cond = crate::evaluator::expressions::eval_expr(&s.cond, env);
    if cond.is_runtime_error() {
        return cond;
    }
    if cond.is_truthy() {
        let scope = Environment::child(env);
        return eval_block(&s.consequence, &scope);
    }
    if let Some(alt) = &s.alternative {
        let scope = Environment::child(env);
        return eval_stmt(alt, &scope);
    }
    Object::Undefined
}

fn eval_while(s: &WhileStmt, env: &EnvRef) -> Object {
    loop {
        if let Some(timeout) = env.borrow().vm.check_timeout(s.pos.clone()) {
            return timeout;
        }
        let cond = crate::evaluator::expressions::eval_expr(&s.cond, env);
        if cond.is_runtime_error() {
            return cond;
        }
        if !cond.is_truthy() {
            break;
        }
        let scope = Environment::child(env);
        let result = eval_block(&s.body, &scope);
        if let Object::Return(r) = &result {
            if let Some(sig) = control_signal(r) {
                if sig == BREAK_SIGNAL {
                    break;
                } else {
                    continue;
                }
            }
            return result;
        }
        if result.is_runtime_error() {
            return result;
        }
    }
    Object::Undefined
}

fn eval_for(s: &ForStmt, env: &EnvRef) -> Object {
    let scope = Environment::child(env);
    if let Some(init) = &s.init {
        let r = eval_stmt(init, &scope);
        if r.is_runtime_error() {
            return r;
        }
    }
    loop {
        if let Some(timeout) = env.borrow().vm.check_timeout(s.pos.clone()) {
            return timeout;
        }
        if let Some(cond) = &s.cond {
            let c = crate::evaluator::expressions::eval_expr(cond, &scope);
            if c.is_runtime_error() {
                return c;
            }
            if !c.is_truthy() {
                break;
            }
        }
        let body_scope = Environment::child(&scope);
        let result = eval_block(&s.body, &body_scope);
        if let Object::Return(r) = &result {
            if let Some(sig) = control_signal(r) {
                if sig == BREAK_SIGNAL {
                    break;
                }
                // continue: fall through to post expression
            } else {
                return result;
            }
        }
        if result.is_runtime_error() {
            return result;
        }
        if let Some(post) = &s.post {
            let _ = crate::evaluator::expressions::eval_expr(post, &scope);
        }
    }
    Object::Undefined
}

fn eval_for_in(s: &ForInStmt, env: &EnvRef) -> Object {
    let iterable = crate::evaluator::expressions::eval_expr(&s.iterable, env);
    if iterable.is_runtime_error() {
        return iterable;
    }
    let keys = iterable_keys(&iterable);
    let scope = Environment::child(env);
    for key in keys {
        if let Some(timeout) = env.borrow().vm.check_timeout(s.pos.clone()) {
            return timeout;
        }
        scope.borrow_mut().set_here(s.name.clone(), str_obj(key));
        let body_scope = Environment::child(&scope);
        let result = eval_block(&s.body, &body_scope);
        if let Object::Return(r) = &result {
            if let Some(sig) = control_signal(r) {
                if sig == BREAK_SIGNAL {
                    break;
                } else {
                    continue;
                }
            }
            return result;
        }
        if result.is_runtime_error() {
            return result;
        }
    }
    Object::Undefined
}

fn eval_for_of(s: &ForOfStmt, env: &EnvRef) -> Object {
    let iterable = crate::evaluator::expressions::eval_expr(&s.iterable, env);
    if iterable.is_runtime_error() {
        return iterable;
    }
    let iterator = crate::evaluator::iterator::get_iterator(&iterable, env, s.pos.clone());
    if iterator.is_runtime_error() {
        return iterator;
    }
    let scope = Environment::child(env);
    loop {
        if let Some(timeout) = env.borrow().vm.check_timeout(s.pos.clone()) {
            return timeout;
        }
        let next = crate::evaluator::iterator::iterator_next(&iterator, env, s.pos.clone());
        if next.is_runtime_error() {
            return next;
        }
        let done = crate::evaluator::iterator::iterator_done(&next, s.pos.clone());
        if done.is_runtime_error() {
            return done;
        }
        if done.is_truthy() {
            break;
        }
        let value = crate::evaluator::iterator::iterator_value(&next, s.pos.clone());
        if value.is_runtime_error() {
            return value;
        }
        scope.borrow_mut().set_here(s.name.clone(), value);
        let body_scope = Environment::child(&scope);
        let result = eval_block(&s.body, &body_scope);
        if let Object::Return(r) = &result {
            if let Some(sig) = control_signal(r) {
                if sig == BREAK_SIGNAL {
                    break;
                } else {
                    continue;
                }
            }
            return result;
        }
        if result.is_runtime_error() {
            return result;
        }
    }
    Object::Undefined
}

/// Extract the own string keys of an iterable value (for-in).
pub fn iterable_keys(obj: &Object) -> Vec<String> {
    match obj {
        Object::Array(a) => (0..a.borrow_mut().elements.len())
            .map(|i| i.to_string())
            .collect(),
        Object::Hash(h) => h
            .borrow_mut()
            .entries
            .iter()
            .map(|(k, _)| k.clone())
            .collect(),
        Object::String(s) => (0..s.chars().count()).map(|i| i.to_string()).collect(),
        Object::Map(m) => m
            .borrow()
            .entries
            .iter()
            .map(|(_, key, _)| key.inspect())
            .collect(),
        Object::Set(s) => s
            .borrow()
            .entries
            .iter()
            .map(|(_, value)| value.inspect())
            .collect(),
        _ => Vec::new(),
    }
}

fn eval_return(s: &ReturnStmt, env: &EnvRef) -> Object {
    match &s.value {
        None => Object::Return(Box::new(Object::Undefined)),
        Some(v) => {
            let val = crate::evaluator::expressions::eval_expr(v, env);
            if val.is_runtime_error() {
                return val;
            }
            Object::Return(Box::new(val))
        }
    }
}

fn eval_throw(s: &ThrowStmt, env: &EnvRef) -> Object {
    let val = crate::evaluator::expressions::eval_expr(&s.value, env);
    if val.is_runtime_error() {
        return val;
    }
    let pos = s.pos.clone();
    match &val {
        Object::Error(e) => {
            let mut data = e.borrow_mut().clone();
            data.runtime = true;
            if data.pos.is_zero() {
                data.pos = pos.clone();
            }
            if data.stack.is_empty() {
                data.stack = if pos.is_zero() {
                    format!("{}: {}", data.name, data.message)
                } else {
                    format!("{}: {}\n    at {}", data.name, data.message, pos)
                };
            }
            Object::Error(Rc::new(RefCell::new(data)))
        }
        other => {
            // Wrap a non-error thrown value; keep a reference for `catch`.
            let e = new_named_error(pos.clone(), "Error", other.inspect());
            if let Object::Error(ed) = &e {
                ed.borrow_mut().thrown = Some(other.clone());
            }
            e
        }
    }
}

fn eval_try(s: &TryStmt, env: &EnvRef) -> Object {
    let try_scope = Environment::child(env);
    let mut result = eval_block(&s.block, &try_scope);
    if result.is_runtime_error() {
        if let Some(catch) = &s.catch {
            let catch_scope = Environment::child(env);
            if !catch.name.is_empty() {
                let bound = if let Object::Error(e) = &result {
                    let mut data = e.borrow_mut().clone();
                    data.runtime = false;
                    Object::Error(Rc::new(RefCell::new(data)))
                } else {
                    result.clone()
                };
                catch_scope.borrow_mut().set_here(catch.name.clone(), bound);
            }
            result = eval_block(&catch.body, &catch_scope);
        }
    }
    if let Some(fin) = &s.finalizer {
        let fin_scope = Environment::child(env);
        let _ = eval_block(fin, &fin_scope);
    }
    result
}

fn eval_func_decl(s: &FuncDecl, env: &EnvRef) -> Object {
    let func = Rc::new(Function {
        name: s.name.clone(),
        params: s.params.clone(),
        body: Rc::new(s.body.clone()),
        env: env.clone(),
        is_async: s.is_async,
        return_t: s.return_t.clone(),
        pos: s.pos.clone(),
        lexical_this: false,
    });
    let val = Object::Function(func);
    env.borrow_mut().set_here(s.name.clone(), val.clone());
    val
}

fn eval_class_decl(s: &ClassDecl, env: &EnvRef) -> Object {
    match crate::evaluator::expressions::build_class(s, env) {
        Ok(cls) => {
            if !s.name.is_empty() {
                env.borrow_mut().set_here(s.name.clone(), cls.clone());
            }
            cls
        }
        Err(e) => e,
    }
}

fn eval_import(s: &ImportDecl, env: &EnvRef) -> Object {
    let importer = env.borrow_mut().vm.importer();
    let source = strip_quotes(&s.source);
    let module = match importer {
        Some(f) => match f(env, &source) {
            Ok(m) => m,
            Err(e) => return e,
        },
        None => {
            return new_error(
                s.pos.clone(),
                "ImportError: module loading is not configured",
            )
        }
    };
    // Bind imported names.
    if !s.default.is_empty() {
        let default = get_member(&module, "default", s.pos.clone());
        env.borrow_mut().set_here(s.default.clone(), default);
    }
    if !s.namespace.is_empty() {
        env.borrow_mut()
            .set_here(s.namespace.clone(), module.clone());
    }
    for name in &s.names {
        let v = get_member(&module, name, s.pos.clone());
        env.borrow_mut().set_here(name.clone(), v);
    }
    for (name, alias) in &s.aliases {
        let v = get_member(&module, name, s.pos.clone());
        env.borrow_mut().set_here(alias.clone(), v);
    }
    Object::Undefined
}

fn eval_export(s: &ExportDecl, env: &EnvRef) -> Object {
    // Exports are tracked on the module's `exports` object (set up by the loader).
    let exports = env.borrow_mut().get("exports").unwrap_or(Object::Undefined);

    // Re-export from another module: `export { a, b as c } from "./m"`.
    if !s.from.is_empty() {
        let importer = env.borrow_mut().vm.importer();
        let source = strip_quotes(&s.from);
        let module = match importer {
            Some(f) => match f(env, &source) {
                Ok(m) => m,
                Err(e) => return e,
            },
            None => {
                return new_error(
                    s.pos.clone(),
                    "ImportError: module loading is not configured",
                )
            }
        };
        for spec in &s.specifiers {
            let v = get_member(&module, &spec.name, s.pos.clone());
            set_member(&exports, &spec.alias, v);
        }
        return Object::Undefined;
    }

    if let Some(decl) = &s.decl {
        let r = eval_stmt(decl, env);
        if r.is_runtime_error() {
            return r;
        }
        // If the declaration was a named binding, export it by name.
        match decl.as_ref() {
            Stmt::FuncDecl(f) => {
                set_member(
                    &exports,
                    &f.name,
                    env.borrow_mut().get(&f.name).unwrap_or(Object::Undefined),
                );
            }
            Stmt::ClassDecl(c) => {
                set_member(
                    &exports,
                    &c.name,
                    env.borrow_mut().get(&c.name).unwrap_or(Object::Undefined),
                );
            }
            Stmt::Let(l) => {
                set_member(
                    &exports,
                    &l.name,
                    env.borrow_mut().get(&l.name).unwrap_or(Object::Undefined),
                );
            }
            Stmt::Const(c) => {
                set_member(
                    &exports,
                    &c.name,
                    env.borrow_mut().get(&c.name).unwrap_or(Object::Undefined),
                );
            }
            Stmt::Var(v) => {
                set_member(
                    &exports,
                    &v.name,
                    env.borrow_mut().get(&v.name).unwrap_or(Object::Undefined),
                );
            }
            Stmt::Expr(_) if s.is_default => {
                if let Stmt::Expr(es) = decl.as_ref() {
                    let v = crate::evaluator::expressions::eval_expr(&es.expr, env);
                    set_member(&exports, "default", v);
                }
            }
            _ => {}
        }
    }
    for spec in &s.specifiers {
        let v = env
            .borrow_mut()
            .get(&spec.name)
            .unwrap_or(Object::Undefined);
        set_member(&exports, &spec.alias, v);
    }
    Object::Undefined
}

/// Read a property of an object by name (used by import/export).
pub fn get_member(obj: &Object, name: &str, pos: Position) -> Object {
    match obj {
        Object::Hash(h) => h
            .borrow_mut()
            .get(name)
            .cloned()
            .unwrap_or(Object::Undefined),
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

/// Set a property on an object by name (used by import/export).
pub fn set_member(obj: &Object, name: &str, value: Object) {
    if let Object::Hash(h) = obj {
        h.borrow_mut().set(name, value);
    }
}

/// Strip surrounding quotes from a string literal token.
pub fn strip_quotes(lit: &str) -> String {
    let b = lit.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        lit[1..b.len() - 1].to_string()
    } else {
        lit.to_string()
    }
}

/// If the object is a control-flow sentinel Return(Error(...)), return the
/// signal string.
pub fn control_signal(obj: &Object) -> Option<&'static str> {
    let signal = match obj {
        Object::Return(r) => r.as_ref(),
        other => other,
    };
    if let Object::Error(e) = signal {
        let msg = &e.borrow_mut().message;
        if msg == BREAK_SIGNAL {
            return Some(BREAK_SIGNAL);
        }
        if msg == CONTINUE_SIGNAL {
            return Some(CONTINUE_SIGNAL);
        }
    }
    None
}
