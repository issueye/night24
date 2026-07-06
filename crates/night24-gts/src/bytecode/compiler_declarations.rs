use crate::ast::{BindingPattern, ConstStmt, Expr, LetStmt, Position, TypeAnnotation, VarStmt};
use crate::object::Object;

use super::chunk::Chunk;
use super::compiler::compile_expr;
use super::compiler_decl_store::decl_name_operand;
use super::compiler_destructuring::compile_destructure;
use super::emit::emit_value_constant;
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

/// Stage 1 keeps all variables in the (root) environment's name table, so a
/// declaration evaluates its initializer (if any) and emits a STORE_NAME.
/// `const` is recorded so a later assignment raises the matching TypeError;
/// the const-ness is tracked by the environment binding, not the opcode.
pub(super) fn compile_let_stmt(
    s: &LetStmt,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    compile_named_or_destructure(
        &s.name,
        s.binding.as_ref(),
        s.value.as_ref(),
        s.type_anno.as_ref(),
        false,
        s.pos.clone(),
        chunk,
        resolutions,
    )
}

pub(super) fn compile_var_stmt(
    s: &VarStmt,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    compile_named_or_destructure(
        &s.name,
        s.binding.as_ref(),
        s.value.as_ref(),
        s.type_anno.as_ref(),
        false,
        s.pos.clone(),
        chunk,
        resolutions,
    )
}

pub(super) fn compile_const_stmt(
    s: &ConstStmt,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    compile_named_or_destructure(
        &s.name,
        s.binding.as_ref(),
        s.value.as_ref(),
        s.type_anno.as_ref(),
        true,
        s.pos.clone(),
        chunk,
        resolutions,
    )
}

fn compile_named_or_destructure(
    name: &str,
    binding: Option<&BindingPattern>,
    value: Option<&Expr>,
    type_anno: Option<&TypeAnnotation>,
    is_const: bool,
    pos: Position,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    if let Some(binding) = binding {
        return compile_destructure(binding, value, pos, is_const, chunk, resolutions);
    }
    compile_decl(name, value, type_anno, is_const, pos, chunk, resolutions)
}

fn compile_decl(
    name: &str,
    value: Option<&Expr>,
    type_anno: Option<&TypeAnnotation>,
    is_const: bool,
    pos: Position,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    if let Some(v) = value {
        compile_expr(v, chunk, resolutions)?;
    } else {
        // Declaration without initializer -> undefined.
        emit_value_constant(chunk, Object::Undefined, pos.clone())?;
    }
    let operand = decl_name_operand(chunk, name, is_const);
    if let Some(type_anno) = type_anno {
        let type_idx = chunk.types.len() as u16;
        chunk.types.push(type_anno.clone());
        chunk.write_op(Opcode::StoreTypedName, pos.clone());
        chunk.write_u16(operand, pos.clone());
        chunk.write_u16(type_idx, pos);
    } else {
        chunk.write_op(Opcode::StoreName, pos.clone());
        chunk.write_u16(operand, pos);
    }
    Ok(())
}
