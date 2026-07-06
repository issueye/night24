use crate::object::Object;

use super::chunk::{Chunk, ProtectedRegion};
use super::compiler::{compile_stmt, FinallyFrame, LoopFrame};
use super::compiler_abrupt::emit_pending_finally_exits;
use super::emit::emit_string_operand;
use super::emit::{emit_jump_placeholder, emit_load_name, patch_jump_here};
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

pub(super) fn compile_try(
    s: &crate::ast::TryStmt,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    finalizers: &mut Vec<FinallyFrame>,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    let has_finalizer = s.finalizer.is_some();
    if let Some(finalizer) = &s.finalizer {
        finalizers.push(FinallyFrame::new(finalizer.clone()));
    }

    let try_start = chunk.code.len() as u32;
    for stmt in &s.block.statements {
        compile_stmt(stmt, chunk, loops, finalizers, false, resolutions)?;
    }
    let try_end = chunk.code.len() as u32;
    let to_normal_finally = emit_jump_placeholder(chunk, Opcode::Jump, s.pos.clone());

    let catch_start = chunk.code.len() as u32;
    let mut catch_end = catch_start;
    if let Some(catch) = &s.catch {
        if catch.name.is_empty() {
            chunk.write_op(Opcode::Pop, catch.pos.clone());
        } else {
            emit_string_operand(
                chunk,
                Opcode::StoreName,
                catch.name.clone(),
                catch.pos.clone(),
            );
        }
        for stmt in &catch.body.statements {
            compile_stmt(stmt, chunk, loops, finalizers, false, resolutions)?;
        }
        catch_end = chunk.code.len() as u32;
    }

    let finally_frame = if has_finalizer {
        Some(finalizers.pop().unwrap())
    } else {
        None
    };

    patch_jump_here(chunk, to_normal_finally);
    if let Some(finalizer) = &s.finalizer {
        for stmt in &finalizer.statements {
            compile_stmt(stmt, chunk, loops, finalizers, false, resolutions)?;
        }
    }

    let to_end = if s.finalizer.is_some() {
        Some(emit_jump_placeholder(chunk, Opcode::Jump, s.pos.clone()))
    } else {
        None
    };

    let exceptional_finally_ip = s.finalizer.as_ref().map(|_| chunk.code.len() as u32);
    if let Some(finalizer) = &s.finalizer {
        let pending_name = format!("__gts_bc_pending_error_{}_{}", s.pos.line, s.pos.col);
        emit_string_operand(
            chunk,
            Opcode::StoreName,
            pending_name.clone(),
            s.pos.clone(),
        );
        for stmt in &finalizer.statements {
            compile_stmt(stmt, chunk, loops, finalizers, false, resolutions)?;
        }
        emit_load_name(chunk, &pending_name, s.pos.clone());
        chunk.write_op(Opcode::Throw, s.pos.clone());
    }

    if let Some(frame) = finally_frame {
        emit_pending_finally_exits(frame, chunk, loops, finalizers, resolutions)?;
    }

    if let Some(end) = to_end {
        patch_jump_here(chunk, end);
    }

    let handler_ip = if s.catch.is_some() {
        catch_start
    } else {
        exceptional_finally_ip.unwrap_or(catch_start)
    };
    chunk.protected_regions.push(ProtectedRegion {
        try_start,
        try_end,
        handler_ip,
        finally_ip: exceptional_finally_ip,
        catch_binding_slot: None,
    });
    if s.finalizer.is_some() && catch_end > catch_start {
        chunk.protected_regions.push(ProtectedRegion {
            try_start: catch_start,
            try_end: catch_end,
            handler_ip: exceptional_finally_ip.unwrap(),
            finally_ip: exceptional_finally_ip,
            catch_binding_slot: None,
        });
    }
    Ok(())
}
