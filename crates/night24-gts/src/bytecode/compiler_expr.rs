use crate::ast::Expr;
use crate::object::Object;

use super::chunk::Chunk;
use super::compiler::{compile_stmt, unsupported};
use super::compiler_access::{
    compile_await_expr, compile_dynamic_import, compile_index_read, compile_member_read,
    compile_super_expr, compile_this_expr,
};
use super::compiler_assign::compile_assign;
use super::compiler_calls::{compile_call, compile_new_expr};
use super::compiler_classes::compile_class_expr;
use super::compiler_collections::{compile_array_lit, compile_object_lit};
use super::compiler_conditionals::{compile_optional, compile_ternary};
use super::compiler_functions::{compile_arrow_expr, compile_func_expr};
use super::compiler_literals::compile_literal_expr;
use super::compiler_operators::{compile_infix_expr, compile_prefix_expr};
use super::emit::emit_string_operand;
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

pub(super) fn compile_expr(
    expr: &Expr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    if let Some(result) = compile_literal_expr(expr, chunk, resolutions, compile_expr) {
        return result;
    }

    match expr {
        // —— identifier read ——
        Expr::Ident(i) => {
            emit_string_operand(chunk, Opcode::LoadName, i.name.clone(), i.pos.clone());
            Ok(())
        }
        // —— assignment `name = expr` (and compound `+=` etc.) ——
        Expr::Assign(a) => compile_assign(a, chunk, resolutions, compile_expr),

        // Literal expressions are handled by `compile_literal_expr` before the
        // main dispatch so this match can stay focused on non-literal forms.
        Expr::Number(_)
        | Expr::Bool(_)
        | Expr::Null(_)
        | Expr::Undefined(_)
        | Expr::String(_)
        | Expr::Regexp(_)
        | Expr::Template(_) => unreachable!("literal expression handled before dispatch"),

        Expr::Match(m) => {
            super::compiler_match::compile_match(m, chunk, resolutions, compile_expr, compile_stmt)
        }
        Expr::DynamicImport(d) => compile_dynamic_import(d, chunk),
        Expr::Await(a) => compile_await_expr(a, chunk, resolutions, compile_expr),
        Expr::Array(a) => compile_array_lit(a, chunk, resolutions, compile_expr),
        Expr::Object(o) => compile_object_lit(o, chunk, resolutions, compile_expr),

        Expr::Prefix(p) => compile_prefix_expr(p, chunk, resolutions, compile_expr),
        Expr::Infix(i) => compile_infix_expr(i, chunk, resolutions, compile_expr),
        Expr::Ternary(t) => compile_ternary(t, chunk, resolutions, compile_expr),

        // —— function call (callee + args, then CALL) ——
        Expr::Call(c) => compile_call(c, chunk, resolutions, compile_expr),
        Expr::Optional(o) => compile_optional(o, chunk, resolutions, compile_expr),
        Expr::Member(m) => compile_member_read(m, chunk, resolutions, compile_expr),
        Expr::Index(i) => compile_index_read(i, chunk, resolutions, compile_expr),
        Expr::New(n) => compile_new_expr(n, chunk, resolutions, compile_expr),
        Expr::This(t) => compile_this_expr(t, chunk),
        Expr::Super(s) => compile_super_expr(s, chunk),
        Expr::Class(c) => compile_class_expr(c, chunk),
        Expr::Func(f) => compile_func_expr(f, chunk, resolutions),
        Expr::Arrow(a) => compile_arrow_expr(a, chunk, resolutions),
        Expr::Spread(sp) => Err(unsupported(
            sp.pos.clone(),
            "bare spread expression outside array/object/call context",
        )),
    }
}
