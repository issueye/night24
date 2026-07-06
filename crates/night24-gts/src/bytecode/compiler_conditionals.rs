use crate::ast::{Expr, InfixExpr, OptionalExpr, Position, TernaryExpr};
use crate::object::Object;

use super::chunk::Chunk;
use super::compiler::unsupported;
use super::compiler_calls::compile_call_args;
use super::compiler_helpers::object_property_key_expr;
use super::emit::emit_string_operand;
use super::emit::{emit_const, emit_jump_placeholder, patch_jump_here, patch_jump_to};
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

type CompileExprFn = fn(&Expr, &mut Chunk, &ResolutionMap) -> Result<(), Object>;

pub(super) fn compile_ternary(
    t: &TernaryExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    compile_expr(&t.cond, chunk, resolutions)?;
    let to_alternate = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, t.pos.clone());
    compile_expr(&t.consequent, chunk, resolutions)?;
    let to_end = emit_jump_placeholder(chunk, Opcode::Jump, t.pos.clone());
    patch_jump_here(chunk, to_alternate);
    compile_expr(&t.alternate, chunk, resolutions)?;
    patch_jump_here(chunk, to_end);
    Ok(())
}

pub(super) fn compile_optional(
    o: &OptionalExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    compile_expr(&o.object, chunk, resolutions)?;
    let nullish_jumps = emit_nullish_jump_checks(chunk, o.pos.clone());

    if o.is_call {
        compile_call_args(
            &o.args,
            false,
            o.pos.clone(),
            chunk,
            resolutions,
            compile_expr,
        )?;
    } else if o.computed {
        compile_expr(&o.property, chunk, resolutions)?;
        chunk.write_op(Opcode::GetIndex, o.pos.clone());
    } else {
        let name = object_property_key_expr(&o.property);
        if name.is_empty() {
            return Err(unsupported(o.pos.clone(), "optional property key"));
        }
        emit_string_operand(chunk, Opcode::GetProperty, name, o.pos.clone());
    }

    let to_end = emit_jump_placeholder(chunk, Opcode::Jump, o.pos.clone());
    let nullish_ip = chunk.code.len() as u32;
    for jump in nullish_jumps {
        patch_jump_to(chunk, jump, nullish_ip);
    }
    chunk.write_op(Opcode::Pop, o.pos.clone());
    let undefined_idx = chunk.add_constant(Object::Undefined);
    emit_const(chunk, undefined_idx, o.pos.clone());
    patch_jump_here(chunk, to_end);
    Ok(())
}

fn emit_nullish_jump_checks(chunk: &mut Chunk, pos: Position) -> Vec<u32> {
    chunk.write_op(Opcode::Dup, pos.clone());
    let null_idx = chunk.add_constant(Object::Null);
    emit_const(chunk, null_idx, pos.clone());
    chunk.write_op(Opcode::Eq, pos.clone());
    let null_jump = emit_jump_placeholder(chunk, Opcode::JumpIfTrue, pos.clone());

    chunk.write_op(Opcode::Dup, pos.clone());
    let undefined_idx = chunk.add_constant(Object::Undefined);
    emit_const(chunk, undefined_idx, pos.clone());
    chunk.write_op(Opcode::Eq, pos.clone());
    let undefined_jump = emit_jump_placeholder(chunk, Opcode::JumpIfTrue, pos);

    vec![null_jump, undefined_jump]
}

/// Lower `left && right`: keep left if falsy, else replace with right.
/// Pre: left is already on the stack.
pub(super) fn compile_and(
    i: &InfixExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    let pos = i.pos.clone();
    //   <left>            ; stack: [L]
    //   DUP               ; stack: [L, L]
    //   JUMP_IF_FALSE end ; pops test, stack: [L]
    //   POP               ; stack: []
    //   <right>           ; stack: [R]
    //   end:
    chunk.write_op(Opcode::Dup, pos.clone());
    let patch_ip = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, pos.clone());
    chunk.write_op(Opcode::Pop, pos.clone());
    compile_expr(i.right.as_ref().unwrap(), chunk, resolutions)?;
    patch_jump_here(chunk, patch_ip);
    Ok(())
}

/// Lower `left || right`: keep left if truthy, else replace with right.
/// Pre: left is already on the stack.
pub(super) fn compile_or(
    i: &InfixExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    let pos = i.pos.clone();
    //   <left>                  ; stack: [L]
    //   DUP                     ; stack: [L, L]
    //   JUMP_IF_FALSE eval_right; pops test, stack: [L]
    //   JUMP end                ; stack: [L] (truthy: keep)
    //   eval_right: POP         ; stack: []
    //   <right>                 ; stack: [R]
    //   end:
    chunk.write_op(Opcode::Dup, pos.clone());
    let to_right = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, pos.clone());
    let to_end = emit_jump_placeholder(chunk, Opcode::Jump, pos.clone());
    patch_jump_here(chunk, to_right);
    chunk.write_op(Opcode::Pop, pos.clone());
    compile_expr(i.right.as_ref().unwrap(), chunk, resolutions)?;
    patch_jump_here(chunk, to_end);
    Ok(())
}

/// Lower `left ?? right`: keep left unless it is null or undefined.
/// Pre: left is already on the stack.
pub(super) fn compile_nullish_coalescing(
    i: &InfixExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    let pos = i.pos.clone();
    //   <left>                       ; stack: [L]
    //   null/undefined checks        ; stack: [L]
    //   JUMP end                     ; stack: [L] (non-nullish: keep)
    //   nullish: POP                 ; stack: []
    //   <right>                      ; stack: [R]
    //   end:
    let nullish_jumps = emit_nullish_jump_checks(chunk, pos.clone());
    let to_end = emit_jump_placeholder(chunk, Opcode::Jump, pos.clone());
    let nullish_ip = chunk.code.len() as u32;
    for jump in nullish_jumps {
        patch_jump_to(chunk, jump, nullish_ip);
    }
    chunk.write_op(Opcode::Pop, pos.clone());
    compile_expr(i.right.as_ref().unwrap(), chunk, resolutions)?;
    patch_jump_here(chunk, to_end);
    Ok(())
}
