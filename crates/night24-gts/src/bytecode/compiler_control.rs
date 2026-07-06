use crate::ast::Stmt;
use crate::object::Object;

use super::chunk::Chunk;
use super::compiler::{
    compile_break_continue, compile_expr, compile_stmt, FinallyFrame, LoopFrame,
};
use super::compiler_iterators::{compile_for_in, compile_for_of};
use super::emit::{emit_jump_placeholder, patch_jump_here, patch_jump_to};
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
