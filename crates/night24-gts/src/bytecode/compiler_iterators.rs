use crate::ast::Expr;
use crate::object::{num_obj, Object};

use super::chunk::Chunk;
use super::compiler::{compile_expr, compile_stmt, FinallyFrame, LoopFrame};
use super::emit::{
    emit_const, emit_jump_placeholder, emit_load_name, emit_string_operand, patch_jump_here,
    patch_jump_to,
};
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_for_in(
    name: &str,
    iterable: &Expr,
    body: &crate::ast::BlockStmt,
    pos: crate::ast::Position,
    label: Option<String>,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    finalizers: &mut Vec<FinallyFrame>,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    let suffix = format!("{}_{}", pos.line, pos.col);
    let items_name = format!("__gts_bc_iter_items_{}", suffix);
    let idx_name = format!("__gts_bc_iter_idx_{}", suffix);

    // items = ITER_KEYS/ITER_VALUES(iterable)
    compile_expr(iterable, chunk, resolutions)?;
    chunk.write_op(Opcode::IterKeys, pos.clone());
    emit_string_operand(chunk, Opcode::StoreName, items_name.clone(), pos.clone());

    // idx = 0
    let zero = chunk.add_constant(num_obj(0.0));
    emit_const(chunk, zero, pos.clone());
    emit_string_operand(chunk, Opcode::StoreName, idx_name.clone(), pos.clone());

    // start: idx < len(items)
    let start = chunk.code.len() as u32;
    emit_load_name(chunk, &idx_name, pos.clone());
    emit_load_name(chunk, &items_name, pos.clone());
    chunk.write_op(Opcode::Len, pos.clone());
    chunk.write_op(Opcode::Lt, pos.clone());
    let to_end = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, pos.clone());

    // loop variable = items[idx]
    emit_load_name(chunk, &items_name, pos.clone());
    emit_load_name(chunk, &idx_name, pos.clone());
    chunk.write_op(Opcode::GetIndex, pos.clone());
    emit_string_operand(chunk, Opcode::StoreName, name, pos.clone());

    let id = loops.len();
    loops.push(LoopFrame {
        id,
        label,
        finalizer_depth: finalizers.len(),
        ..LoopFrame::default()
    });
    for stmt in &body.statements {
        compile_stmt(stmt, chunk, loops, finalizers, false, resolutions)?;
    }
    let frame = loops.pop().unwrap();

    // continue target: idx = idx + 1
    let increment = chunk.code.len() as u32;
    emit_load_name(chunk, &idx_name, pos.clone());
    let one = chunk.add_constant(num_obj(1.0));
    emit_const(chunk, one, pos.clone());
    chunk.write_op(Opcode::Add, pos.clone());
    chunk.write_op(Opcode::Dup, pos.clone());
    emit_string_operand(chunk, Opcode::AssignName, idx_name, pos.clone());
    chunk.write_op(Opcode::Pop, pos.clone());
    chunk.write_op(Opcode::Loop, pos.clone());
    chunk.write_u32(start, pos.clone());

    let end = chunk.code.len() as u32;
    patch_jump_here(chunk, to_end);
    for b in &frame.breaks {
        patch_jump_to(chunk, *b, end);
    }
    for c in &frame.continues {
        patch_jump_to(chunk, *c, increment);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_for_of(
    name: &str,
    iterable: &Expr,
    body: &crate::ast::BlockStmt,
    pos: crate::ast::Position,
    label: Option<String>,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    finalizers: &mut Vec<FinallyFrame>,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    let suffix = format!("{}_{}", pos.line, pos.col);
    let iter_name = format!("__gts_bc_iter_{}", suffix);
    let next_name = format!("__gts_bc_iter_next_{}", suffix);

    compile_expr(iterable, chunk, resolutions)?;
    chunk.write_op(Opcode::IterValues, pos.clone());
    emit_string_operand(chunk, Opcode::StoreName, iter_name.clone(), pos.clone());

    let start = chunk.code.len() as u32;
    emit_load_name(chunk, &iter_name, pos.clone());
    chunk.write_op(Opcode::IterNext, pos.clone());
    emit_string_operand(chunk, Opcode::StoreName, next_name.clone(), pos.clone());

    emit_load_name(chunk, &next_name, pos.clone());
    emit_string_operand(chunk, Opcode::GetProperty, "done", pos.clone());
    let to_end = emit_jump_placeholder(chunk, Opcode::JumpIfTrue, pos.clone());

    emit_load_name(chunk, &next_name, pos.clone());
    emit_string_operand(chunk, Opcode::GetProperty, "value", pos.clone());
    emit_string_operand(chunk, Opcode::StoreName, name, pos.clone());

    let id = loops.len();
    loops.push(LoopFrame {
        id,
        label,
        finalizer_depth: finalizers.len(),
        ..LoopFrame::default()
    });
    for stmt in &body.statements {
        compile_stmt(stmt, chunk, loops, finalizers, false, resolutions)?;
    }
    let frame = loops.pop().unwrap();

    chunk.write_op(Opcode::Loop, pos.clone());
    chunk.write_u32(start, pos.clone());

    let end = chunk.code.len() as u32;
    patch_jump_here(chunk, to_end);
    for b in &frame.breaks {
        patch_jump_to(chunk, *b, end);
    }
    for c in &frame.continues {
        patch_jump_to(chunk, *c, start);
    }
    Ok(())
}
