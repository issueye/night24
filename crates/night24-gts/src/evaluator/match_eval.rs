//! `match` expression evaluation.

use crate::ast::*;
use crate::object::*;

use super::eval_core::eval_block;
use super::expressions::eval_expr;

pub fn eval_match(m: &MatchExpr, env: &EnvRef) -> Object {
    let subject = eval_expr(&m.expr, env);
    if subject.is_runtime_error() {
        return subject;
    }
    for arm in &m.arms {
        let scope = Environment::child(env);
        if match_pattern(&arm.pattern, &subject, &scope) {
            if !arm.binding_name.is_empty() {
                scope
                    .borrow_mut()
                    .set_here(arm.binding_name.clone(), subject.clone());
            }
            if let Some(guard) = &arm.guard {
                let g = eval_expr(guard, &scope);
                if !g.is_truthy() {
                    continue;
                }
            }
            return match &arm.body {
                MatchBody::Expr(e) => eval_expr(e, &scope),
                MatchBody::Block(b) => eval_block(b, &scope),
            };
        }
    }
    new_error(
        m.pos.clone(),
        format!("MatchError: no arm matched for {}", subject.inspect()),
    )
}

fn match_pattern(pat: &Pattern, value: &Object, scope: &EnvRef) -> bool {
    match pat {
        Pattern::Literal(lp) => {
            // Evaluate the literal pattern value in a throwaway scope.
            let empty = Environment::new_root(scope.borrow_mut().vm.clone());
            let v = eval_expr(&lp.value, &empty);
            strict_equal(&v, value)
        }
        Pattern::Ident(ip) => {
            scope.borrow_mut().set_here(ip.name.clone(), value.clone());
            true
        }
        Pattern::Wildcard(_) => true,
        Pattern::Or(op) => op
            .alternatives
            .iter()
            .any(|a| match_pattern(a, value, scope)),
        Pattern::Range(rp) => {
            let empty = Environment::new_root(scope.borrow_mut().vm.clone());
            let start = eval_expr(&rp.start, &empty);
            let end = eval_expr(&rp.end, &empty);
            let (Object::Number(v), Object::Number(s), Object::Number(e)) = (value, &start, &end)
            else {
                return false;
            };
            if rp.inclusive {
                *v >= *s && *v <= *e
            } else {
                *v >= *s && *v < *e
            }
        }
    }
}
