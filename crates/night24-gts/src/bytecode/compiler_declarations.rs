use crate::ast::{BindingPattern, ConstStmt, Expr, LetStmt, Position, TypeAnnotation, VarStmt};
use crate::object::{num_obj, str_obj, Object};

use super::chunk::Chunk;
use super::compiler::compile_expr;
use super::emit::{emit_const, emit_jump_placeholder, patch_jump_here};
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
        let idx = chunk.add_constant(Object::Undefined);
        emit_const(chunk, idx, pos.clone());
    }
    let name_idx = chunk.add_constant(str_obj(name));
    // Encode const-ness in the high bit of the name index operand so the
    // interpreter knows which binding flavor to create. (Name pools stay
    // small; a u16 with a flag bit is plenty.)
    let operand = if is_const {
        name_idx | 0x8000
    } else {
        name_idx
    };
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

/// Compile a destructuring declaration: evaluate the source once, then bind
/// each element via Dup-source + GetIndex/GetProperty (+ default) + Store.
/// `is_const` flags const-ness in each StoreName operand.
fn compile_destructure(
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
        let idx = chunk.add_constant(Object::Undefined);
        emit_const(chunk, idx, pos.clone());
    }

    let store = |name: &str, chunk: &mut Chunk, pos: &Position| {
        let name_idx = chunk.add_constant(str_obj(name.to_string()));
        let operand = if is_const {
            name_idx | 0x8000
        } else {
            name_idx
        };
        chunk.write_op(Opcode::StoreName, pos.clone());
        chunk.write_u16(operand, pos.clone());
    };

    match binding {
        BindingPattern::Array(elems) => {
            for (i, elem) in elems.iter().enumerate() {
                if elem.is_rest {
                    // `...rest`: collect elements [i..] into a new array.
                    chunk.write_op(Opcode::Dup, pos.clone());
                    let start_const = chunk.add_constant(num_obj(i as f64));
                    chunk.write_op(Opcode::Const, pos.clone());
                    chunk.write_u16(start_const, pos.clone());
                    chunk.write_op(Opcode::ArraySliceFrom, pos.clone());
                    store(&elem.name, chunk, &pos);
                    break;
                }
                if elem.name.is_empty() {
                    continue;
                }
                chunk.write_op(Opcode::Dup, pos.clone());
                let idx_const = chunk.add_constant(num_obj(i as f64));
                chunk.write_op(Opcode::Const, pos.clone());
                chunk.write_u16(idx_const, pos.clone());
                chunk.write_op(Opcode::GetIndex, pos.clone());
                if let Some(def) = &elem.default {
                    emit_undefined_replace(def, chunk, resolutions, &pos)?;
                }
                store(&elem.name, chunk, &pos);
            }
        }
        BindingPattern::Object(elems) => {
            for elem in elems {
                chunk.write_op(Opcode::Dup, pos.clone());
                let key_idx = chunk.add_constant(str_obj(elem.key.clone()));
                chunk.write_op(Opcode::GetProperty, pos.clone());
                chunk.write_u16(key_idx, pos.clone());
                if let Some(def) = &elem.default {
                    emit_undefined_replace(def, chunk, resolutions, &pos)?;
                }
                store(&elem.target, chunk, &pos);
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
    let und = chunk.add_constant(Object::Undefined);
    chunk.write_op(Opcode::Const, pos.clone());
    chunk.write_u16(und, pos.clone());
    chunk.write_op(Opcode::Eq, pos.clone());
    let keep_ip = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, pos.clone());
    chunk.write_op(Opcode::Pop, pos.clone());
    compile_expr(default, chunk, resolutions)?;
    patch_jump_here(chunk, keep_ip);
    Ok(())
}
