//! Expression evaluation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::*;
use crate::object::*;

use super::eval_core::eval_block;
use super::match_eval::eval_match;
use super::string_lit::{eval_regexp_lit, eval_string_lit, eval_template};

/// Evaluate any expression.
pub fn eval_expr(expr: &Expr, env: &EnvRef) -> Object {
    if let Some(timeout) = env.borrow().vm.check_timeout(expr.pos()) {
        return timeout;
    }
    match expr {
        Expr::Ident(i) => eval_ident(i, env),
        Expr::DynamicImport(d) => {
            // Tree-walker parity: load the module and wrap in a resolved Promise.
            let source = match &d.source {
                Expr::String(s) => eval_string_lit(s),
                Expr::Template(t) => eval_template(t, env),
                _ => Object::Undefined,
            };
            let specifier = match &source {
                Object::String(s) => s.to_string(),
                _ => String::new(),
            };
            let module = if let Some(importer) = env.borrow().vm.importer() {
                match importer(env, &specifier) {
                    Ok(m) => m,
                    Err(e) => return e,
                }
            } else {
                Object::Undefined
            };
            let promise = crate::object::Promise::new();
            promise.resolve(module);
            Object::Promise(promise)
        }
        Expr::Number(n) => Object::Number(n.value),
        Expr::String(s) => eval_string_lit(s),
        Expr::Template(t) => eval_template(t, env),
        Expr::Regexp(r) => eval_regexp_lit(r),
        Expr::Bool(b) => Object::Boolean(b.value),
        Expr::Null(_) => Object::Null,
        Expr::Undefined(_) => Object::Undefined,
        Expr::This(_) => env.borrow_mut().this.clone().unwrap_or(Object::Undefined),
        Expr::Super(s) => eval_super(s, env),
        Expr::Array(a) => eval_array(a, env),
        Expr::Object(o) => eval_object_lit(o, env),
        Expr::Prefix(p) => eval_prefix(p, env),
        Expr::Infix(i) => eval_infix(i, env),
        Expr::Ternary(t) => eval_ternary(t, env),
        Expr::Assign(a) => eval_assign(a, env),
        Expr::Call(c) => eval_call(c, env),
        Expr::Member(m) => eval_member(m, env),
        Expr::Index(i) => eval_index(i, env),
        Expr::Optional(o) => eval_optional(o, env),
        Expr::Func(f) => eval_func_expr(f, env),
        Expr::Arrow(a) => eval_arrow(a, env),
        Expr::New(n) => eval_new(n, env),
        Expr::Await(a) => eval_await(a, env),
        Expr::Spread(s) => eval_expr(&s.value, env),
        Expr::Match(m) => eval_match(m, env),
        Expr::Class(c) => match build_class(c, env) {
            Ok(cls) => cls,
            Err(e) => e,
        },
    }
}

fn eval_ident(i: &Ident, env: &EnvRef) -> Object {
    match env.borrow_mut().get(&i.name) {
        Some(v) => v,
        None => new_error(
            i.pos.clone(),
            format!("ReferenceError: '{}' is not defined", i.name),
        ),
    }
}

fn eval_array(a: &ArrayLit, env: &EnvRef) -> Object {
    let mut elems = Vec::new();
    for e in &a.elements {
        if let Expr::Spread(sp) = e {
            let val = eval_expr(&sp.value, env);
            match &val {
                Object::Array(arr) => elems.extend(arr.borrow_mut().elements.iter().cloned()),
                _ => elems.push(val),
            }
        } else {
            let v = eval_expr(e, env);
            if v.is_runtime_error() {
                return v;
            }
            elems.push(v);
        }
    }
    Object::Array(Rc::new(RefCell::new(ArrayData { elements: elems })))
}

fn eval_object_lit(o: &ObjectLit, env: &EnvRef) -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    for p in &o.properties {
        if p.spread {
            let val = eval_expr(&p.value, env);
            if let Object::Hash(h) = &val {
                for (k, v) in h.borrow_mut().entries.iter() {
                    hash.borrow_mut().set(k.clone(), v.clone());
                }
            }
            continue;
        }
        let key = if p.computed {
            let k = eval_expr(&p.key, env);
            if k.is_runtime_error() {
                return k;
            }
            k.inspect()
        } else {
            property_key(&p.key)
        };
        let val = eval_expr(&p.value, env);
        if val.is_runtime_error() {
            return val;
        }
        hash.borrow_mut().set(key, val);
    }
    Object::Hash(hash)
}

fn property_key(e: &Expr) -> String {
    match e {
        Expr::Ident(i) => i.name.clone(),
        Expr::String(s) => super::eval_core::strip_quotes(&s.literal),
        Expr::Number(n) => format_number(n.value),
        _ => String::new(),
    }
}

// ============================================================================
// Prefix / Infix
// ============================================================================

fn eval_prefix(p: &PrefixExpr, env: &EnvRef) -> Object {
    if p.op == "++" || p.op == "--" {
        return eval_update(&p.right, env, &p.op, false, p.pos.clone());
    }
    let right = eval_expr(&p.right, env);
    if right.is_runtime_error() {
        return right;
    }
    // `delete` is statement-like and always returns true; route the rest
    // through the shared unary-op core.
    if p.op == "delete" {
        return Object::Boolean(true);
    }
    apply_unary_op(p.op.as_str(), &right, p.pos.clone())
}

pub fn typeof_name(value: &Object) -> String {
    match value {
        Object::Undefined => "undefined".into(),
        Object::Null => "object".into(),
        Object::Boolean(_) => "boolean".into(),
        Object::Number(_) => "number".into(),
        Object::String(_) => "string".into(),
        Object::Function(_) | Object::Builtin(_) | Object::Class(_) | Object::Closure(_) => {
            "function".into()
        }
        _ => "object".into(),
    }
}

fn eval_infix(i: &InfixExpr, env: &EnvRef) -> Object {
    if (i.op == "++" || i.op == "--") && i.right.is_none() {
        return eval_update(&i.left, env, &i.op, true, i.pos.clone());
    }
    // Past the postfix ++/-- early return above, every other infix operator
    // has a right operand (the parser guarantees it). Bind it once, safely,
    // instead of `.unwrap()`-ing the Option at each use site.
    let right = match &i.right {
        Some(r) => r,
        None => {
            return new_error(
                i.pos.clone(),
                format!("SyntaxError: '{}' is missing its right-hand operand", i.op),
            )
        }
    };
    let left = eval_expr(&i.left, env);
    if left.is_runtime_error() {
        return left;
    }
    match i.op.as_str() {
        "&&" => {
            if !left.is_truthy() {
                return left;
            }
            return eval_expr(right, env);
        }
        "||" => {
            if left.is_truthy() {
                return left;
            }
            return eval_expr(right, env);
        }
        "??" => {
            if !matches!(left, Object::Null | Object::Undefined) {
                return left;
            }
            return eval_expr(right, env);
        }
        _ => {}
    }
    let right_val = eval_expr(right, env);
    if right_val.is_runtime_error() {
        return right_val;
    }
    apply_binary_op(i.op.as_str(), &left, &right_val, i.pos.clone())
}

/// Apply a binary operator to two already-evaluated operands.
///
/// This is the pure (non-short-circuit) core of `eval_infix`, factored out so
/// the bytecode VM can reuse the exact same semantics byte-for-byte without
/// duplicating logic. Short-circuit operators (`&&` / `||` / `??`) are handled
/// by the caller (tree-walker evaluates the right operand lazily; the VM
/// lowers them to conditional jumps) and must NOT be routed through here.
pub fn apply_binary_op(op: &str, left: &Object, right: &Object, pos: Position) -> Object {
    match op {
        "+" => eval_add(left, right, pos),
        "-" => number_op(left, right, pos, |a, b| a - b),
        "*" => number_op(left, right, pos, |a, b| a * b),
        "/" => number_op(left, right, pos, |a, b| a / b),
        "%" => number_op(left, right, pos, |a, b| a.rem_euclid(b)),
        "**" => number_op(left, right, pos, |a, b| a.powf(b)),
        "&" => bit_op(left, right, pos, |a, b| a & b),
        "|" => bit_op(left, right, pos, |a, b| a | b),
        "^" => bit_op(left, right, pos, |a, b| a ^ b),
        "<<" => bit_op(left, right, pos, |a, b| a.wrapping_shl(b as u32)),
        ">>" => bit_op(left, right, pos, |a, b| a.wrapping_shr(b as u32)),
        ">>>" => bit_op(left, right, pos, |a, b| {
            ((a as u32).wrapping_shr(b as u32)) as i32
        }),
        "===" => Object::Boolean(strict_equal(left, right)),
        "!==" => Object::Boolean(!strict_equal(left, right)),
        "<" => compare(left, right, "<", pos),
        "<=" => compare(left, right, "<=", pos),
        ">" => compare(left, right, ">", pos),
        ">=" => compare(left, right, ">=", pos),
        "instanceof" => eval_instanceof(left, right),
        "in" => eval_in(left, right, pos),
        _ => new_error(pos, format!("unknown infix operator: {}", op)),
    }
}

/// Apply a unary prefix operator to an already-evaluated operand.
///
/// Pure core of `eval_prefix`, shared with the bytecode VM. Excludes
/// `++`/`--` (which are update operators handled separately with assignment).
pub fn apply_unary_op(op: &str, right: &Object, pos: Position) -> Object {
    match op {
        "!" => Object::Boolean(!right.is_truthy()),
        "-" => match right {
            Object::Number(n) => Object::Number(-*n),
            _ => new_error(
                pos,
                format!("TypeError: cannot negate {}", right.type_tag()),
            ),
        },
        "+" => match right {
            Object::Number(n) => Object::Number(*n),
            _ => new_error(
                pos,
                format!("TypeError: cannot apply + to {}", right.type_tag()),
            ),
        },
        "typeof" => Object::String(Rc::new(typeof_name(right))),
        "void" => Object::Undefined,
        "~" => match right {
            Object::Number(n) => Object::Number(!(*n as i32) as f64),
            _ => new_error(
                pos,
                format!("TypeError: cannot apply ~ to {}", right.type_tag()),
            ),
        },
        _ => new_error(pos, format!("unknown prefix operator: {}", op)),
    }
}

fn eval_add(left: &Object, right: &Object, pos: Position) -> Object {
    if let (Object::Number(a), Object::Number(b)) = (left, right) {
        return Object::Number(a + b);
    }
    if let (Object::String(a), Object::String(b)) = (left, right) {
        return str_obj(format!("{}{}", a, b));
    }
    if left.is_string() || right.is_string() {
        let other = if left.is_string() {
            right.type_tag()
        } else {
            left.type_tag()
        };
        return new_error(
            pos,
            format!(
                "TypeError: cannot add string and {} — use template literals or String()",
                other
            ),
        );
    }
    new_error(
        pos,
        format!(
            "TypeError: cannot add {} and {} — types must match",
            left.type_tag(),
            right.type_tag()
        ),
    )
}

fn number_op(left: &Object, right: &Object, pos: Position, f: impl Fn(f64, f64) -> f64) -> Object {
    let l = match to_numeric(left) {
        Some(v) => v,
        None => {
            return new_error(
                pos,
                format!(
                    "TypeError: left operand must be number, got {}",
                    left.type_tag()
                ),
            )
        }
    };
    let r = match to_numeric(right) {
        Some(v) => v,
        None => {
            return new_error(
                pos,
                format!(
                    "TypeError: right operand must be number, got {}",
                    right.type_tag()
                ),
            )
        }
    };
    Object::Number(f(l, r))
}

fn to_numeric(obj: &Object) -> Option<f64> {
    match obj {
        Object::Number(n) => Some(*n),
        Object::Date(ms) => Some(*ms as f64),
        _ => None,
    }
}

/// 整数位运算辅助函数。
/// 按 JS ToInt32 语义转换操作数后执行位运算，结果再转回 f64。
fn bit_op(left: &Object, right: &Object, pos: Position, f: impl Fn(i32, i32) -> i32) -> Object {
    let l = match to_numeric(left) {
        Some(v) => to_int32(v),
        None => {
            return new_error(
                pos,
                format!(
                    "TypeError: left operand must be number, got {}",
                    left.type_tag()
                ),
            )
        }
    };
    let r = match to_numeric(right) {
        Some(v) => to_int32(v),
        None => {
            return new_error(
                pos,
                format!(
                    "TypeError: right operand must be number, got {}",
                    right.type_tag()
                ),
            )
        }
    };
    Object::Number(f(l, r) as f64)
}

/// JS ToInt32 转换语义。
fn to_int32(n: f64) -> i32 {
    if !n.is_finite() || n == 0.0 {
        return 0;
    }
    let n = n.trunc();
    let modulo = 4294967296.0; // 2^32
    let m = n - (n / modulo).floor() * modulo; // 正余数
    let m = if m >= 2147483648.0 { m - modulo } else { m };
    m as i32
}

fn compare(left: &Object, right: &Object, op: &str, pos: Position) -> Object {
    let ord = match (left, right) {
        (Object::Number(a), Object::Number(b)) => a.partial_cmp(b),
        (Object::String(a), Object::String(b)) => a.partial_cmp(b),
        _ => {
            return new_error(
                pos,
                format!(
                    "TypeError: cannot compare {} and {} — types must match",
                    left.type_tag(),
                    right.type_tag()
                ),
            )
        }
    };
    let result = match (op, ord) {
        ("<", Some(o)) => o.is_lt(),
        ("<=", Some(o)) => !o.is_gt(),
        (">", Some(o)) => o.is_gt(),
        (">=", Some(o)) => !o.is_lt(),
        _ => false,
    };
    Object::Boolean(result)
}

fn eval_in(left: &Object, right: &Object, pos: Position) -> Object {
    let key = match left {
        Object::String(s) => s.to_string(),
        _ => return new_error(pos, "TypeError: left operand of 'in' must be string"),
    };
    match right {
        Object::Hash(h) => Object::Boolean(h.borrow_mut().contains(&key)),
        Object::Array(a) => {
            if let Ok(i) = key.parse::<usize>() {
                Object::Boolean(i < a.borrow_mut().elements.len())
            } else {
                Object::Boolean(false)
            }
        }
        Object::Instance(i) => {
            let inst = i.borrow_mut();
            Object::Boolean(
                inst.props.contains_key(&key) || inst.class.borrow_mut().methods.contains_key(&key),
            )
        }
        _ => new_error(pos, "TypeError: right operand of 'in' must be object"),
    }
}

fn eval_instanceof(left: &Object, right: &Object) -> Object {
    if let (Object::Instance(inst), Object::Class(cls)) = (left, right) {
        let mut current = Some(inst.borrow_mut().class.clone());
        while let Some(c) = current {
            if Rc::ptr_eq(&c, cls) {
                return Object::Boolean(true);
            }
            current = c.borrow_mut().super_.clone();
        }
        return Object::Boolean(false);
    }
    Object::Boolean(false)
}

// ============================================================================
// Ternary / Assign / Update
// ============================================================================

fn eval_ternary(t: &TernaryExpr, env: &EnvRef) -> Object {
    let cond = eval_expr(&t.cond, env);
    if cond.is_runtime_error() {
        return cond;
    }
    if cond.is_truthy() {
        eval_expr(&t.consequent, env)
    } else {
        eval_expr(&t.alternate, env)
    }
}

fn eval_assign(a: &AssignExpr, env: &EnvRef) -> Object {
    let mut right = eval_expr(&a.right, env);
    if right.is_runtime_error() {
        return right;
    }
    // Compound assignment to a non-identifier target: read current, combine.
    if a.op != "=" && !matches!(a.left, Expr::Ident(_)) {
        let current = eval_expr(&a.left, env);
        if current.is_runtime_error() {
            return current;
        }
        right = compound(&current, &right, &a.op, a.pos.clone());
        if right.is_error() {
            return right;
        }
    }
    match &a.left {
        Expr::Ident(i) => {
            if a.op == "=" {
                let (found, is_const) = env.borrow_mut().assign(&i.name, right.clone());
                if !found {
                    return new_error(
                        i.pos.clone(),
                        format!("ReferenceError: '{}' is not defined", i.name),
                    );
                }
                if is_const {
                    return new_error(
                        i.pos.clone(),
                        format!("TypeError: assignment to constant '{}'", i.name),
                    );
                }
                return right;
            }
            // compound on identifier
            let existing = env.borrow_mut().get(&i.name);
            let cur = match existing {
                Some(v) => v,
                None => {
                    return new_error(
                        i.pos.clone(),
                        format!("ReferenceError: '{}' is not defined", i.name),
                    )
                }
            };
            let combined = compound(&cur, &right, &a.op, a.pos.clone());
            if combined.is_error() {
                return combined;
            }
            let (found, is_const) = env.borrow_mut().assign(&i.name, combined.clone());
            if !found {
                return new_error(
                    i.pos.clone(),
                    format!("ReferenceError: '{}' is not defined", i.name),
                );
            }
            if is_const {
                return new_error(
                    i.pos.clone(),
                    format!("TypeError: assignment to constant '{}'", i.name),
                );
            }
            combined
        }
        Expr::Member(m) => {
            let obj = eval_expr(&m.object, env);
            if obj.is_runtime_error() {
                return obj;
            }
            let name = match &m.property {
                Expr::Ident(id) => id.name.clone(),
                _ => property_key(&m.property),
            };
            // Both arms of the original `if a.op == "=" { right } else { right }`
            // produce the same value, so the branch is redundant.
            let value = right;
            assign_member(&obj, &name, value.clone(), m.pos.clone())
        }
        Expr::Index(idx) => {
            let obj = eval_expr(&idx.left, env);
            if obj.is_runtime_error() {
                return obj;
            }
            let key = eval_expr(&idx.index, env);
            if key.is_runtime_error() {
                return key;
            }
            assign_index(&obj, &key, right.clone(), idx.pos.clone())
        }
        _ => new_error(a.pos.clone(), "SyntaxError: invalid assignment target"),
    }
}

fn assign_member(obj: &Object, name: &str, value: Object, pos: Position) -> Object {
    match obj {
        Object::Hash(h) => {
            if h.borrow_mut().frozen {
                return new_error(pos, "TypeError: cannot assign to frozen object");
            }
            if h.borrow_mut().sealed && !h.borrow_mut().contains(name) {
                return new_error(pos, "TypeError: cannot add property to sealed object");
            }
            h.borrow_mut().set(name, value.clone());
            value
        }
        Object::Instance(i) => {
            i.borrow_mut().props.insert(name.into(), value.clone());
            value
        }
        Object::Class(c) => {
            c.borrow_mut().statics.insert(name.into(), value.clone());
            value
        }
        _ => new_error(
            pos,
            format!("TypeError: cannot assign to property of {}", obj.type_tag()),
        ),
    }
}

fn assign_index(obj: &Object, key: &Object, value: Object, pos: Position) -> Object {
    match obj {
        Object::Array(a) => {
            if let Object::Number(n) = key {
                let i = *n as isize;
                let mut arr = a.borrow_mut();
                let len = arr.elements.len() as isize;
                if i < 0 || i >= len {
                    return new_error(pos, "RangeError: array index out of bounds");
                }
                arr.elements[i as usize] = value.clone();
            }
            value
        }
        Object::Hash(h) => {
            let k = key.inspect();
            if h.borrow_mut().frozen {
                return new_error(pos, "TypeError: cannot assign to frozen object");
            }
            h.borrow_mut().set(k, value.clone());
            value
        }
        _ => new_error(pos, format!("TypeError: cannot index {}", obj.type_tag())),
    }
}

fn compound(left: &Object, right: &Object, op: &str, pos: Position) -> Object {
    if let (Object::Number(a), Object::Number(b)) = (left, right) {
        return match op {
            "+=" => Object::Number(a + b),
            "-=" => Object::Number(a - b),
            "*=" => Object::Number(a * b),
            "/=" => Object::Number(a / b),
            "%=" => Object::Number(a.rem_euclid(*b)),
            _ => new_error(pos, format!("unknown compound op {}", op)),
        };
    }
    if let (Object::String(a), Object::String(b)) = (left, right) {
        if op == "+=" {
            return str_obj(format!("{}{}", a, b));
        }
    }
    new_error(
        pos,
        "TypeError: compound assignment requires matching types",
    )
}

fn eval_update(target: &Expr, env: &EnvRef, op: &str, postfix: bool, pos: Position) -> Object {
    let current = match target {
        Expr::Ident(i) => match env.borrow_mut().get(&i.name) {
            Some(v) => v,
            None => {
                return new_error(
                    i.pos.clone(),
                    format!("ReferenceError: '{}' is not defined", i.name),
                )
            }
        },
        Expr::Member(m) => {
            let obj = eval_expr(&m.object, env);
            let name = match &m.property {
                Expr::Ident(id) => id.name.clone(),
                _ => property_key(&m.property),
            };
            super::methods::get_property(&obj, &name, m.pos.clone())
        }
        Expr::Index(idx) => {
            let obj = eval_expr(&idx.left, env);
            let key = eval_expr(&idx.index, env);
            super::methods::get_index(&obj, &key, idx.pos.clone())
        }
        _ => return new_error(pos, "SyntaxError: invalid update target"),
    };
    let n = match &current {
        Object::Number(n) => *n,
        _ => {
            return new_error(
                pos,
                format!(
                    "TypeError: update operator requires number, got {}",
                    current.type_tag()
                ),
            )
        }
    };
    let delta = if op == "--" { -1.0 } else { 1.0 };
    let next = Object::Number(n + delta);
    // Write back
    match target {
        Expr::Ident(i) => {
            let (found, is_const) = env.borrow_mut().assign(&i.name, next.clone());
            if !found {
                return new_error(
                    i.pos.clone(),
                    format!("ReferenceError: '{}' is not defined", i.name),
                );
            }
            if is_const {
                return new_error(
                    i.pos.clone(),
                    format!("TypeError: assignment to constant '{}'", i.name),
                );
            }
        }
        Expr::Member(m) => {
            let obj = eval_expr(&m.object, env);
            let name = match &m.property {
                Expr::Ident(id) => id.name.clone(),
                _ => property_key(&m.property),
            };
            assign_member(&obj, &name, next.clone(), m.pos.clone());
        }
        Expr::Index(idx) => {
            let obj = eval_expr(&idx.left, env);
            let key = eval_expr(&idx.index, env);
            assign_index(&obj, &key, next.clone(), idx.pos.clone());
        }
        _ => {}
    }
    if postfix {
        current
    } else {
        next
    }
}

// ============================================================================
// Call / Member / Index / New
// ============================================================================

fn eval_call(c: &CallExpr, env: &EnvRef) -> Object {
    // super(...) constructor call
    if let Expr::Super(_) = &c.callee {
        let mut args = Vec::new();
        for a in &c.args {
            args.push(eval_expr(a, env));
        }
        return super::methods::call_super_constructor(env, &args, c.pos.clone());
    }
    // Method call: obj.method(...) / obj[key](...) — capture `this`.
    let (callee, this_val) = match &c.callee {
        Expr::Member(m) => {
            // super.method(...): resolve against the parent class while keeping
            // the current instance as `this`. This must happen here rather than
            // in the generic Member path because `super` as a standalone value
            // has no callable form (eval_super returns Undefined).
            if let Expr::Super(s) = &m.object {
                if let Expr::Ident(id) = &m.property {
                    let this = env.borrow().this.clone();
                    let func = super::methods::get_super_method(env, &id.name, s.pos.clone());
                    (func, this)
                } else {
                    let key = property_key(&m.property);
                    let pos = s.pos.clone();
                    let this = env.borrow().this.clone();
                    let func = super::methods::get_super_method(env, &key, pos);
                    (func, this)
                }
            } else {
                let obj = eval_expr(&m.object, env);
                if obj.is_runtime_error() {
                    return obj;
                }
                let name = match &m.property {
                    Expr::Ident(id) => id.name.clone(),
                    _ => property_key(&m.property),
                };
                let func = super::methods::get_property(&obj, &name, c.pos.clone());
                (func, Some(obj))
            }
        }
        Expr::Index(i) => {
            let obj = eval_expr(&i.left, env);
            if obj.is_runtime_error() {
                return obj;
            }
            let key = eval_expr(&i.index, env);
            if key.is_runtime_error() {
                return key;
            }
            let func = super::methods::get_index(&obj, &key, c.pos.clone());
            (func, Some(obj))
        }
        _ => (eval_expr(&c.callee, env), None),
    };
    if callee.is_runtime_error() {
        return callee;
    }
    let mut args = Vec::with_capacity(c.args.len());
    for a in &c.args {
        if let Expr::Spread(sp) = a {
            let val = eval_expr(&sp.value, env);
            if let Object::Array(arr) = &val {
                args.extend(arr.borrow_mut().elements.iter().cloned());
            } else {
                args.push(val);
            }
        } else {
            let v = eval_expr(a, env);
            if v.is_runtime_error() {
                return v;
            }
            args.push(v);
        }
    }
    apply_function(&callee, env, &args, this_val, c.pos.clone())
}

/// Apply a callable to arguments, binding `this` if given.
pub fn apply_function(
    func: &Object,
    caller_env: &EnvRef,
    args: &[Object],
    this: Option<Object>,
    pos: Position,
) -> Object {
    match func {
        Object::Function(f) => {
            let scope = Environment::child(&f.env);
            // Bind `this`: explicit (method call) > lexical (arrow) > undefined.
            if let Some(t) = this {
                scope.borrow_mut().this = Some(t);
            } else if !f.lexical_this {
                scope.borrow_mut().this = None;
            }
            if f.is_async {
                return super::methods::run_async_function(f, &scope, caller_env, args, pos);
            }
            if let Err(e) = bind_params(&scope, caller_env, &f.params, args, pos.clone()) {
                return e;
            }
            let result = eval_block(&f.body, &scope);
            match result {
                Object::Return(r) => *r,
                other => other,
            }
        }
        Object::Builtin(b) => {
            let mut ctx = CallContext::new(caller_env, pos);
            ctx.receiver = b.extra.clone();
            (b.func)(&mut ctx, args)
        }
        Object::Closure(c) => {
            crate::bytecode::call::call_closure_object(c.clone(), caller_env, args, pos)
        }
        Object::Class(cls) => super::methods::construct_class(cls, caller_env, args, pos),
        Object::Hash(h) => {
            // Callable object with __call.
            if let Some(Object::Builtin(b)) = h.borrow_mut().get("__call").cloned() {
                let mut ctx = CallContext::new(caller_env, pos);
                return (b.func)(&mut ctx, args);
            }
            new_error(pos, "TypeError: object is not a function")
        }
        _ => new_error(
            pos,
            format!("TypeError: {} is not a function", func.type_tag()),
        ),
    }
}

/// Bind function parameters (with defaults and rest) into the call scope.
pub fn bind_params(
    scope: &EnvRef,
    caller: &EnvRef,
    params: &[Param],
    args: &[Object],
    pos: Position,
) -> Result<(), Object> {
    for (i, p) in params.iter().enumerate() {
        let value = if i < args.len() {
            if p.spread {
                let rest: Vec<Object> = args[i..].to_vec();
                let arr = Object::Array(Rc::new(RefCell::new(ArrayData { elements: rest })));
                scope.borrow_mut().set_here(p.name.clone(), arr);
                break;
            }
            args[i].clone()
        } else if let Some(def) = &p.default {
            let v = eval_expr(def, scope);
            if v.is_runtime_error() {
                return Err(v);
            }
            v
        } else {
            Object::Undefined
        };
        let _ = (caller, &pos);
        scope.borrow_mut().set_here(p.name.clone(), value);
    }
    bind_arguments_object(scope, args);
    Ok(())
}

fn bind_arguments_object(scope: &EnvRef, args: &[Object]) {
    let already_bound = scope.borrow().bindings.contains_key("arguments");
    if already_bound {
        return;
    }
    let elements = args.to_vec();
    let arguments = Object::Array(Rc::new(RefCell::new(ArrayData { elements })));
    scope.borrow_mut().set_here("arguments", arguments);
}

fn eval_member(m: &MemberExpr, env: &EnvRef) -> Object {
    // super.method
    if let Expr::Super(s) = &m.object {
        if let Expr::Ident(id) = &m.property {
            return super::methods::get_super_method(env, &id.name, s.pos.clone());
        }
    }
    let obj = eval_expr(&m.object, env);
    if obj.is_runtime_error() {
        return obj;
    }
    let name = match &m.property {
        Expr::Ident(id) => id.name.clone(),
        _ => property_key(&m.property),
    };
    super::methods::get_property(&obj, &name, m.pos.clone())
}

fn eval_index(i: &IndexExpr, env: &EnvRef) -> Object {
    let left = eval_expr(&i.left, env);
    if left.is_runtime_error() {
        return left;
    }
    let idx = eval_expr(&i.index, env);
    if idx.is_runtime_error() {
        return idx;
    }
    super::methods::get_index(&left, &idx, i.pos.clone())
}

fn eval_optional(o: &OptionalExpr, env: &EnvRef) -> Object {
    let obj = eval_expr(&o.object, env);
    if matches!(obj, Object::Null | Object::Undefined) {
        return Object::Undefined;
    }
    if o.is_call {
        let mut args = Vec::new();
        for a in &o.args {
            args.push(eval_expr(a, env));
        }
        return apply_function(&obj, env, &args, None, o.pos.clone());
    }
    match &o.property {
        Expr::Ident(id) => super::methods::get_property(&obj, &id.name, o.pos.clone()),
        _ => {
            let key = eval_expr(&o.property, env);
            super::methods::get_index(&obj, &key, o.pos.clone())
        }
    }
}

fn eval_func_expr(f: &FuncExpr, env: &EnvRef) -> Object {
    let func = Rc::new(Function {
        name: f.name.clone(),
        params: f.params.clone(),
        body: Rc::new(f.body.clone()),
        env: env.clone(),
        is_async: f.is_async,
        return_t: f.return_t.clone(),
        pos: f.pos.clone(),
        lexical_this: false,
    });
    Object::Function(func)
}

fn eval_arrow(a: &ArrowFuncExpr, env: &EnvRef) -> Object {
    // Capture the current `this` lexically.
    let captured_this = env.borrow_mut().this.clone();
    let body = match &a.body {
        ArrowBody::Expr(e) => BlockStmt {
            pos: a.pos.clone(),
            statements: vec![Stmt::Return(ReturnStmt {
                pos: a.pos.clone(),
                value: Some(e.clone()),
            })],
        },
        ArrowBody::Block(b) => b.clone(),
    };
    let func = Rc::new(Function {
        name: String::new(),
        params: a.params.clone(),
        body: Rc::new(body),
        env: env.clone(),
        is_async: a.is_async,
        return_t: a.return_t.clone(),
        pos: a.pos.clone(),
        lexical_this: true,
    });
    // Stash captured this on the function env so call-time binding restores it.
    // We encode lexical this by pre-setting `this` on a fresh child at call;
    // instead we store it in the function's env via a hidden slot is not trivial.
    // Simpler: arrow functions never receive a `this` arg (apply_function skips
    // binding when lexical_this is true), so we must inject the captured value.
    // We do that by setting `this` on the env the closure captures.
    if let Some(t) = captured_this {
        func.env.borrow_mut().this = Some(t);
    }
    Object::Function(func)
}

fn eval_new(n: &NewExpr, env: &EnvRef) -> Object {
    let callee = eval_expr(&n.callee, env);
    if callee.is_runtime_error() {
        return callee;
    }
    let mut args = Vec::with_capacity(n.args.len());
    for a in &n.args {
        args.push(eval_expr(a, env));
    }
    match &callee {
        Object::Class(cls) => super::methods::construct_class(cls, env, &args, n.pos.clone()),
        Object::Builtin(b) => {
            // Builtin constructors like Error/Date/Map/Set/Promise/RegExp.
            super::methods::construct_builtin(b, env, &args, n.pos.clone())
        }
        Object::Function(f) => {
            // new on a function: treat like apply with a new instance-less call.
            apply_function(
                &Object::Function(f.clone()),
                env,
                &args,
                None,
                n.pos.clone(),
            )
        }
        Object::Hash(_) => apply_function(&callee, env, &args, None, n.pos.clone()),
        _ => new_error(
            n.pos.clone(),
            format!("TypeError: {} is not a constructor", callee.type_tag()),
        ),
    }
}

fn eval_await(a: &AwaitExpr, env: &EnvRef) -> Object {
    let val = eval_expr(&a.value, env);
    match &val {
        Object::Promise(p) => {
            if p.state() == PromiseState::Pending {
                env.borrow().vm.wait_async();
            }
            let result = p.wait();
            if p.state() == PromiseState::Rejected {
                // Surface the rejection as a runtime error.
                match &result {
                    Object::Error(e) => {
                        let mut data = e.borrow_mut().clone();
                        data.runtime = true;
                        if data.pos.is_zero() {
                            data.pos = a.pos.clone();
                        }
                        Object::Error(Rc::new(RefCell::new(data)))
                    }
                    other => new_error(a.pos.clone(), other.inspect()),
                }
            } else {
                result
            }
        }
        _ => val,
    }
}

fn eval_super(s: &SuperExpr, env: &EnvRef) -> Object {
    if !s.method.is_empty() {
        return super::methods::get_super_method(env, &s.method, s.pos.clone());
    }
    Object::Undefined
}

// ============================================================================
// Class construction
// ============================================================================

/// Build a class value from a class declaration.
pub fn build_class(s: &ClassDecl, env: &EnvRef) -> Result<Object, Object> {
    let mut class = Class {
        name: s.name.clone(),
        super_: None,
        methods: HashMap::new(),
        fields: HashMap::new(),
        field_types: HashMap::new(),
        statics: HashMap::new(),
        static_types: HashMap::new(),
        native_ctor: None,
        pos: s.pos.clone(),
    };
    if let Some(super_expr) = &s.super_ {
        let sv = eval_expr(super_expr, env);
        match &sv {
            Object::Class(sc) => {
                class.super_ = Some(sc.clone());
                // Inherit non-constructor methods and fields.
                let scb = sc.borrow_mut();
                for (k, v) in scb.methods.iter() {
                    if k != "constructor" {
                        class.methods.insert(k.clone(), v.clone());
                    }
                }
                for (k, v) in scb.fields.iter() {
                    class.fields.insert(k.clone(), v.clone());
                }
            }
            Object::Builtin(b) if is_error_class_name(&b.name) => {
                class.super_ = Some(native_error_class(env, &b.name, s.pos.clone())?);
            }
            _ => {
                return Err(new_error(
                    s.pos.clone(),
                    "TypeError: superclass must be a class",
                ))
            }
        }
    }
    for m in &s.body.members {
        match m.kind {
            ClassMemberKind::Method | ClassMemberKind::Constructor => {
                let body = match &m.body {
                    Some(b) => b.clone(),
                    None => continue,
                };
                let func = Rc::new(Function {
                    name: m.name.clone(),
                    params: m.params.clone(),
                    body: Rc::new(body),
                    env: env.clone(),
                    is_async: m.is_async,
                    return_t: m.type_anno.clone(),
                    pos: m.pos.clone(),
                    lexical_this: false,
                });
                if m.is_static {
                    class.statics.insert(m.name.clone(), Object::Function(func));
                } else {
                    class.methods.insert(m.name.clone(), Object::Function(func));
                }
            }
            ClassMemberKind::Field => {
                let val = match &m.default_val {
                    Some(e) => {
                        let v = eval_expr(e, env);
                        if v.is_runtime_error() {
                            return Err(v);
                        }
                        v
                    }
                    None => Object::Undefined,
                };
                if m.is_static {
                    class.statics.insert(m.name.clone(), val);
                    if let Some(t) = &m.type_anno {
                        class.static_types.insert(m.name.clone(), t.clone());
                    }
                } else {
                    class.fields.insert(m.name.clone(), val);
                    if let Some(t) = &m.type_anno {
                        class.field_types.insert(m.name.clone(), t.clone());
                    }
                }
            }
        }
    }
    Ok(Object::Class(Rc::new(RefCell::new(class))))
}

pub fn is_error_class_name(name: &str) -> bool {
    matches!(
        name,
        "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
    )
}

pub(crate) fn native_error_class(
    _env: &EnvRef,
    name: &str,
    pos: Position,
) -> Result<Rc<RefCell<Class>>, Object> {
    let class = Class {
        name: name.into(),
        super_: None,
        methods: HashMap::new(),
        fields: HashMap::new(),
        field_types: HashMap::new(),
        statics: HashMap::new(),
        static_types: HashMap::new(),
        native_ctor: Some(Rc::new({
            let name = name.to_string();
            move |_ctx: &mut CallContext, inst: &Rc<RefCell<Instance>>, args: &[Object]| -> Object {
                let message = if let Some(m) = args.first() {
                    m.inspect()
                } else {
                    String::new()
                };
                let err = new_named_error(Position::default(), &name, &message);
                if let Object::Error(e) = &err {
                    let ed = e.borrow_mut();
                    inst.borrow_mut()
                        .props
                        .insert("name".into(), str_obj(ed.name.clone()));
                    inst.borrow_mut()
                        .props
                        .insert("message".into(), str_obj(ed.message.clone()));
                    inst.borrow_mut()
                        .props
                        .insert("stack".into(), str_obj(ed.stack.clone()));
                }
                Object::Undefined
            }
        })),
        pos,
    };
    Ok(Rc::new(RefCell::new(class)))
}
