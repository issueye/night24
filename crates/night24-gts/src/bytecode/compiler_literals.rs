use crate::ast::Expr;
use crate::evaluator::string_lit::{eval_regexp_lit, eval_string_lit};
use crate::object::{bool_obj, num_obj, Object};

use super::chunk::Chunk;
use super::compiler_templates::compile_template_literal;
use super::emit::{emit_const, emit_value_constant};
use super::resolve::ResolutionMap;

type CompileExprFn = fn(&Expr, &mut Chunk, &ResolutionMap) -> Result<(), Object>;

pub(super) fn compile_literal_expr(
    expr: &Expr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Option<Result<(), Object>> {
    Some(match expr {
        Expr::Number(n) => emit_value_constant(chunk, num_obj(n.value), n.pos.clone()),
        Expr::Bool(b) => emit_value_constant(chunk, bool_obj(b.value), b.pos.clone()),
        Expr::Null(n) => emit_value_constant(chunk, Object::Null, n.pos.clone()),
        Expr::Undefined(u) => emit_value_constant(chunk, Object::Undefined, u.pos.clone()),
        Expr::String(s) => compile_string_literal(s, chunk),
        Expr::Regexp(r) => compile_regexp_literal(r, chunk),
        Expr::Template(t) => compile_template_literal(t, chunk, resolutions, compile_expr),
        _ => return None,
    })
}

fn compile_string_literal(s: &crate::ast::StringLit, chunk: &mut Chunk) -> Result<(), Object> {
    // String literals are pure (escape processing only, no env), so evaluate
    // them at compile time and intern the result.
    let value = eval_string_lit(s);
    if value.is_runtime_error() {
        return Err(value);
    }
    let idx = chunk.add_constant(value);
    emit_const(chunk, idx, s.pos.clone());
    Ok(())
}

fn compile_regexp_literal(r: &crate::ast::RegExpLit, chunk: &mut Chunk) -> Result<(), Object> {
    // Regexp literals compile to a RegexpData value (pure).
    let value = eval_regexp_lit(r);
    if value.is_runtime_error() {
        return Err(value);
    }
    let idx = chunk.add_constant(value);
    emit_const(chunk, idx, r.pos.clone());
    Ok(())
}
