use crate::ast::{AwaitExpr, DynamicImportExpr, Expr, IndexExpr, MemberExpr, SuperExpr, ThisExpr};
use crate::object::Object;

use super::chunk::Chunk;
use super::compiler::unsupported;
use super::compiler_helpers::object_property_key_expr;
use super::emit::{emit_const, emit_string_operand};
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

type CompileExprFn = fn(&Expr, &mut Chunk, &ResolutionMap) -> Result<(), Object>;

pub(super) fn compile_dynamic_import(
    d: &DynamicImportExpr,
    chunk: &mut Chunk,
) -> Result<(), Object> {
    let source = match &d.source {
        Expr::String(s) => crate::evaluator::eval_core::strip_quotes(&s.literal),
        Expr::Template(t) => crate::evaluator::eval_core::strip_quotes(&t.literal),
        _ => {
            return Err(unsupported(
                d.pos.clone(),
                "dynamic import() requires a string specifier",
            ));
        }
    };
    emit_string_operand(chunk, Opcode::ImportModule, source, d.pos.clone());
    chunk.write_op(Opcode::WrapResolvedPromise, d.pos.clone());
    Ok(())
}

pub(super) fn compile_await_expr(
    a: &AwaitExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    compile_expr(&a.value, chunk, resolutions)?;
    chunk.write_op(Opcode::Await, a.pos.clone());
    Ok(())
}

pub(super) fn compile_member_read(
    m: &MemberExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    compile_expr(&m.object, chunk, resolutions)?;
    if m.computed {
        compile_expr(&m.property, chunk, resolutions)?;
        chunk.write_op(Opcode::GetIndex, m.pos.clone());
    } else {
        let name = object_property_key_expr(&m.property);
        if name.is_empty() {
            return Err(unsupported(m.pos.clone(), "member property key"));
        }
        emit_string_operand(chunk, Opcode::GetProperty, name, m.pos.clone());
    }
    Ok(())
}

pub(super) fn compile_index_read(
    i: &IndexExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    compile_expr(&i.left, chunk, resolutions)?;
    compile_expr(&i.index, chunk, resolutions)?;
    chunk.write_op(Opcode::GetIndex, i.pos.clone());
    Ok(())
}

pub(super) fn compile_this_expr(t: &ThisExpr, chunk: &mut Chunk) -> Result<(), Object> {
    chunk.write_op(Opcode::LoadThis, t.pos.clone());
    Ok(())
}

pub(super) fn compile_super_expr(s: &SuperExpr, chunk: &mut Chunk) -> Result<(), Object> {
    if s.method.is_empty() {
        let idx = chunk.add_constant(Object::Undefined);
        emit_const(chunk, idx, s.pos.clone());
    } else {
        emit_string_operand(chunk, Opcode::SuperMethod, s.method.clone(), s.pos.clone());
    }
    Ok(())
}
