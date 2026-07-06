use crate::ast::{Expr, Stmt};
use crate::object::{num_obj, str_obj, Object};

use super::chunk::Chunk;
use super::compiler::{
    compile_break_continue, compile_expr, compile_stmt, FinallyFrame, LoopFrame,
};
use super::emit::{
    emit_const, emit_jump_placeholder, emit_load_name, patch_jump_here, patch_jump_to,
};
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

/// Compile `if (cond) { ... } else { ... }`.
pub(super) fn compile_if(
    s: &crate::ast::IfStmt,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    finalizers: &mut Vec<FinallyFrame>,
    _keep_value: bool,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    // cond ; JUMP_IF_FALSE else ; <then> ; JUMP end ; else: <else> ; end:
    compile_expr(&s.cond, chunk, resolutions)?;
    let to_else = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, s.pos.clone());
    for stmt in &s.consequence.statements {
        compile_stmt(stmt, chunk, loops, finalizers, false, resolutions)?;
    }
    let to_end = if s.alternative.is_some() {
        Some(emit_jump_placeholder(chunk, Opcode::Jump, s.pos.clone()))
    } else {
        None
    };
    patch_jump_here(chunk, to_else);
    if let Some(alt) = &s.alternative {
        compile_stmt(alt, chunk, loops, finalizers, false, resolutions)?;
    }
    if let Some(end) = to_end {
        patch_jump_here(chunk, end);
    }
    Ok(())
}

/// Compile `while (cond) { body }`.
pub(super) fn compile_while(
    s: &crate::ast::WhileStmt,
    label: Option<String>,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    finalizers: &mut Vec<FinallyFrame>,
    _keep_value: bool,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    // start: cond ; JUMP_IF_FALSE end ; <body> ; LOOP start ; end:
    let start = chunk.code.len() as u32;
    compile_expr(&s.cond, chunk, resolutions)?;
    let to_end = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, s.pos.clone());
    let id = loops.len();
    loops.push(LoopFrame {
        id,
        label,
        finalizer_depth: finalizers.len(),
        ..LoopFrame::default()
    });
    for stmt in &s.body.statements {
        compile_stmt(stmt, chunk, loops, finalizers, false, resolutions)?;
    }
    let frame = loops.pop().unwrap();
    // Back-edge: LOOP to the condition test.
    chunk.write_op(Opcode::Loop, s.pos.clone());
    chunk.write_u32(start, s.pos.clone());
    let end = chunk.code.len() as u32;
    patch_jump_here(chunk, to_end);
    // Patch break/continue jumps collected in the frame.
    for b in &frame.breaks {
        patch_jump_to(chunk, *b, end);
    }
    for c in &frame.continues {
        patch_jump_to(chunk, *c, start);
    }
    Ok(())
}

/// Compile `for (init; cond; post) { body }`.
pub(super) fn compile_for(
    s: &crate::ast::ForStmt,
    label: Option<String>,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    finalizers: &mut Vec<FinallyFrame>,
    _keep_value: bool,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    // <init> ; start: <cond> ; JUMP_IF_FALSE end ; <body> ; post_start: <post> ; LOOP start ; end:
    if let Some(init) = &s.init {
        compile_stmt(init, chunk, loops, finalizers, false, resolutions)?;
    }
    let start = chunk.code.len() as u32;
    let mut to_end: Option<u32> = None;
    if let Some(cond) = &s.cond {
        compile_expr(cond, chunk, resolutions)?;
        to_end = Some(emit_jump_placeholder(
            chunk,
            Opcode::JumpIfFalse,
            s.pos.clone(),
        ));
    }
    let id = loops.len();
    loops.push(LoopFrame {
        id,
        label,
        finalizer_depth: finalizers.len(),
        ..LoopFrame::default()
    });
    for stmt in &s.body.statements {
        compile_stmt(stmt, chunk, loops, finalizers, false, resolutions)?;
    }
    let frame = loops.pop().unwrap();
    // post expression (continue targets here) - recorded AFTER the body so its
    // offset is correct.
    let post_start = chunk.code.len() as u32;
    if let Some(post) = &s.post {
        compile_expr(post, chunk, resolutions)?;
        chunk.write_op(Opcode::Pop, s.pos.clone()); // discard post value
    }
    chunk.write_op(Opcode::Loop, s.pos.clone());
    chunk.write_u32(start, s.pos.clone());
    let end = chunk.code.len() as u32;
    if let Some(end_patch) = to_end {
        patch_jump_here(chunk, end_patch);
    }
    for b in &frame.breaks {
        patch_jump_to(chunk, *b, end);
    }
    for c in &frame.continues {
        patch_jump_to(chunk, *c, post_start);
    }
    Ok(())
}

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
    let items_idx = chunk.add_constant(str_obj(items_name.clone()));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(items_idx, pos.clone());

    // idx = 0
    let zero = chunk.add_constant(num_obj(0.0));
    emit_const(chunk, zero, pos.clone());
    let idx_idx = chunk.add_constant(str_obj(idx_name.clone()));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(idx_idx, pos.clone());

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
    let name_idx = chunk.add_constant(str_obj(name.to_string()));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(name_idx, pos.clone());

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
    let idx_idx = chunk.add_constant(str_obj(idx_name));
    chunk.write_op(Opcode::AssignName, pos.clone());
    chunk.write_u16(idx_idx, pos.clone());
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
    let iter_idx = chunk.add_constant(str_obj(iter_name.clone()));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(iter_idx, pos.clone());

    let start = chunk.code.len() as u32;
    emit_load_name(chunk, &iter_name, pos.clone());
    chunk.write_op(Opcode::IterNext, pos.clone());
    let next_idx = chunk.add_constant(str_obj(next_name.clone()));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(next_idx, pos.clone());

    emit_load_name(chunk, &next_name, pos.clone());
    let done_idx = chunk.add_constant(str_obj("done"));
    chunk.write_op(Opcode::GetProperty, pos.clone());
    chunk.write_u16(done_idx, pos.clone());
    let to_end = emit_jump_placeholder(chunk, Opcode::JumpIfTrue, pos.clone());

    emit_load_name(chunk, &next_name, pos.clone());
    let value_idx = chunk.add_constant(str_obj("value"));
    chunk.write_op(Opcode::GetProperty, pos.clone());
    chunk.write_u16(value_idx, pos.clone());
    let name_idx = chunk.add_constant(str_obj(name.to_string()));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(name_idx, pos.clone());

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

pub(super) fn compile_labeled(
    s: &crate::ast::LabeledStmt,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    finalizers: &mut Vec<FinallyFrame>,
    keep_value: bool,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    match s.stmt.as_ref() {
        Stmt::While(w) => compile_while(
            w,
            Some(s.label.clone()),
            chunk,
            loops,
            finalizers,
            keep_value,
            resolutions,
        ),
        Stmt::For(f) => compile_for(
            f,
            Some(s.label.clone()),
            chunk,
            loops,
            finalizers,
            keep_value,
            resolutions,
        ),
        Stmt::ForIn(f) => compile_for_in(
            &f.name,
            &f.iterable,
            &f.body,
            f.pos.clone(),
            Some(s.label.clone()),
            chunk,
            loops,
            finalizers,
            resolutions,
        ),
        Stmt::ForOf(f) => compile_for_of(
            &f.name,
            &f.iterable,
            &f.body,
            f.pos.clone(),
            Some(s.label.clone()),
            chunk,
            loops,
            finalizers,
            resolutions,
        ),
        Stmt::Break(b) => {
            compile_break_continue(true, &b.label, b.pos.clone(), chunk, loops, finalizers)
        }
        other => compile_stmt(other, chunk, loops, finalizers, keep_value, resolutions),
    }
}
