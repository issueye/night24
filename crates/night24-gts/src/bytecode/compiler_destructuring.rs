use crate::ast::{BindingPattern, Expr, Position};
use crate::object::{num_obj, Object};

use super::chunk::Chunk;
use super::compiler::compile_expr;
use super::compiler_decl_store::emit_decl_store;
use super::emit::{
    emit_jump_placeholder, emit_string_operand, emit_value_constant, patch_jump_here,
};
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

/// Compile a destructuring declaration: evaluate the source once, then bind
/// each element via Dup-source + GetIndex/GetProperty (+ default) + Store.
/// `is_const` flags const-ness in each StoreName operand.
pub(super) fn compile_destructure(
    binding: &BindingPattern,
    value: Option<&Expr>,
    pos: Position,
    is_const: bool,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    // Evaluate the source once; it stays on the stack across all bindings.
    if let Some(v) = value {
        compile_expr(v, chunk, resolutions)?;
    } else {
        emit_value_constant(chunk, Object::Undefined, pos.clone())?;
    }

    match binding {
        BindingPattern::Array(elems) => {
            for (i, elem) in elems.iter().enumerate() {
                if elem.is_rest {
                    // `...rest`: collect elements [i..] into a new array.
                    chunk.write_op(Opcode::Dup, pos.clone());
                    emit_value_constant(chunk, num_obj(i as f64), pos.clone())?;
                    chunk.write_op(Opcode::ArraySliceFrom, pos.clone());
                    emit_decl_store(chunk, &elem.name, is_const, pos.clone());
                    break;
                }
                if elem.name.is_empty() {
                    continue;
                }
                chunk.write_op(Opcode::Dup, pos.clone());
                emit_value_constant(chunk, num_obj(i as f64), pos.clone())?;
                chunk.write_op(Opcode::GetIndex, pos.clone());
                if let Some(def) = &elem.default {
                    emit_undefined_replace(def, chunk, resolutions, &pos)?;
                }
                emit_decl_store(chunk, &elem.name, is_const, pos.clone());
            }
        }
        BindingPattern::Object(elems) => {
            for elem in elems {
                chunk.write_op(Opcode::Dup, pos.clone());
                emit_string_operand(chunk, Opcode::GetProperty, elem.key.clone(), pos.clone());
                if let Some(def) = &elem.default {
                    emit_undefined_replace(def, chunk, resolutions, &pos)?;
                }
                emit_decl_store(chunk, &elem.target, is_const, pos.clone());
            }
        }
    }
    // Drop the original source.
    chunk.write_op(Opcode::Pop, pos);
    Ok(())
}

/// If the value on top of the stack is `undefined`, replace it with the
/// compiled default expression; otherwise keep it.
fn emit_undefined_replace(
    default: &Expr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    pos: &Position,
) -> Result<(), Object> {
    chunk.write_op(Opcode::Dup, pos.clone());
    emit_value_constant(chunk, Object::Undefined, pos.clone())?;
    chunk.write_op(Opcode::Eq, pos.clone());
    let keep_ip = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, pos.clone());
    chunk.write_op(Opcode::Pop, pos.clone());
    compile_expr(default, chunk, resolutions)?;
    patch_jump_here(chunk, keep_ip);
    Ok(())
}
