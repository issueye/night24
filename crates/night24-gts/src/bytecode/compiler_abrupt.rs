use crate::object::Object;

use super::chunk::Chunk;
use super::compiler::{compile_expr, compile_stmt, unsupported};
use super::emit::emit_string_operand;
use super::emit::{emit_const, emit_jump_placeholder, emit_load_name, patch_jump_here};
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

/// A loop being compiled: holds patch sites for `break` (jump to end) and
/// `continue` (jump to the post-expression / condition re-test).
#[derive(Default)]
pub(super) struct LoopFrame {
    pub(super) id: usize,
    /// Optional label attached to this loop.
    pub(super) label: Option<String>,
    /// Number of active finally blocks that enclose this loop. A break/continue
    /// targeting this loop must run only the finalizers nested inside it.
    pub(super) finalizer_depth: usize,
    /// Byte offsets of pending `break` jumps (each is a JUMP placeholder).
    pub(super) breaks: Vec<u32>,
    /// Byte offsets of pending `continue` jumps.
    pub(super) continues: Vec<u32>,
}

pub(super) struct FinallyFrame {
    finalizer: crate::ast::BlockStmt,
    exits: Vec<PendingFinallyExit>,
}

impl FinallyFrame {
    pub(super) fn new(finalizer: crate::ast::BlockStmt) -> Self {
        Self {
            finalizer,
            exits: Vec::new(),
        }
    }
}

#[derive(Clone)]
struct PendingFinallyExit {
    patch: u32,
    pos: crate::ast::Position,
    action: AbruptAction,
}

#[derive(Clone)]
enum AbruptAction {
    Return {
        temp_name: String,
    },
    Break {
        loop_id: usize,
        finalizer_depth: usize,
    },
    Continue {
        loop_id: usize,
        finalizer_depth: usize,
    },
}

pub(super) fn compile_return_stmt(
    r: &crate::ast::ReturnStmt,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    finalizers: &mut Vec<FinallyFrame>,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    if let Some(v) = &r.value {
        compile_expr(v, chunk, resolutions)?;
    } else {
        let idx = chunk.add_constant(Object::Undefined);
        emit_const(chunk, idx, r.pos.clone());
    }
    if finalizers.is_empty() {
        chunk.write_op(Opcode::Return, r.pos.clone());
        Ok(())
    } else {
        let temp_name = format!("__gts_bc_return_{}_{}", r.pos.line, r.pos.col);
        emit_string_operand(chunk, Opcode::StoreName, temp_name.clone(), r.pos.clone());
        emit_abrupt_action(
            AbruptAction::Return { temp_name },
            r.pos.clone(),
            chunk,
            loops,
            finalizers,
        )
    }
}

/// Compile `break` (is_break=true) or `continue`. Records a pending JUMP in
/// the current loop frame to be patched when the loop's end / continue-target
/// is known.
#[allow(clippy::ptr_arg)]
pub(super) fn compile_break_continue(
    is_break: bool,
    label: &str,
    pos: crate::ast::Position,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    finalizers: &mut Vec<FinallyFrame>,
) -> Result<(), Object> {
    let Some(target) = loops.iter().rev().find(|f| {
        label.is_empty()
            || f.label
                .as_ref()
                .map(|frame_label| frame_label == label)
                .unwrap_or(false)
    }) else {
        return Err(unsupported(
            pos,
            if label.is_empty() {
                if is_break {
                    "break outside loop"
                } else {
                    "continue outside loop"
                }
            } else if is_break {
                "labeled break target"
            } else {
                "labeled continue target"
            },
        ));
    };
    let action = if is_break {
        AbruptAction::Break {
            loop_id: target.id,
            finalizer_depth: target.finalizer_depth,
        }
    } else {
        AbruptAction::Continue {
            loop_id: target.id,
            finalizer_depth: target.finalizer_depth,
        }
    };
    emit_abrupt_action(action, pos, chunk, loops, finalizers)
}

pub(super) fn emit_pending_finally_exits(
    frame: FinallyFrame,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    finalizers: &mut Vec<FinallyFrame>,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    for pending in frame.exits {
        patch_jump_here(chunk, pending.patch);
        for stmt in &frame.finalizer.statements {
            compile_stmt(stmt, chunk, loops, finalizers, false, resolutions)?;
        }
        emit_abrupt_action(pending.action, pending.pos, chunk, loops, finalizers)?;
    }
    Ok(())
}

fn emit_abrupt_action(
    action: AbruptAction,
    pos: crate::ast::Position,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    finalizers: &mut Vec<FinallyFrame>,
) -> Result<(), Object> {
    let target_depth = match &action {
        AbruptAction::Return { .. } => 0,
        AbruptAction::Break {
            finalizer_depth, ..
        }
        | AbruptAction::Continue {
            finalizer_depth, ..
        } => *finalizer_depth,
    };

    if finalizers.len() > target_depth {
        let patch = emit_jump_placeholder(chunk, Opcode::Jump, pos.clone());
        finalizers
            .last_mut()
            .expect("active finalizer")
            .exits
            .push(PendingFinallyExit { patch, pos, action });
        return Ok(());
    }

    match action {
        AbruptAction::Return { temp_name } => {
            emit_load_name(chunk, &temp_name, pos.clone());
            chunk.write_op(Opcode::Return, pos);
        }
        AbruptAction::Break { loop_id, .. } | AbruptAction::Continue { loop_id, .. } => {
            let patch = emit_jump_placeholder(chunk, Opcode::Jump, pos.clone());
            let Some(frame) = loops.iter_mut().rev().find(|frame| frame.id == loop_id) else {
                return Err(unsupported(pos, "loop control target"));
            };
            if matches!(action, AbruptAction::Break { .. }) {
                frame.breaks.push(patch);
            } else {
                frame.continues.push(patch);
            }
        }
    }
    Ok(())
}
