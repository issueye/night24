use crate::ast::{Expr, MatchBody, MatchExpr, Pattern, Stmt};
use crate::object::{bool_obj, str_obj, Object};

use super::chunk::Chunk;
use super::compiler::{FinallyFrame, LoopFrame};
use super::emit::{emit_const, emit_jump_placeholder, emit_load_name, patch_jump_here};
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

type CompileExprFn = fn(&Expr, &mut Chunk, &ResolutionMap) -> Result<(), Object>;
type CompileStmtFn = fn(
    &Stmt,
    &mut Chunk,
    &mut Vec<LoopFrame>,
    &mut Vec<FinallyFrame>,
    bool,
    &ResolutionMap,
) -> Result<(), Object>;

pub(super) fn compile_match(
    m: &MatchExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
    compile_stmt: CompileStmtFn,
) -> Result<(), Object> {
    let subject_name = format!("__gts_bc_match_subject_{}_{}", m.pos.line, m.pos.col);
    compile_expr(&m.expr, chunk, resolutions)?;
    let subject_idx = chunk.add_constant(str_obj(subject_name.clone()));
    chunk.write_op(Opcode::StoreName, m.pos.clone());
    chunk.write_u16(subject_idx, m.pos.clone());

    let mut to_end = Vec::new();
    for arm in &m.arms {
        emit_load_name(chunk, &subject_name, arm.pos.clone());
        compile_pattern_test(&arm.pattern, chunk, resolutions, compile_expr)?;
        let to_next = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, arm.pos.clone());

        if let Pattern::Ident(ip) = &arm.pattern {
            emit_load_name(chunk, &subject_name, ip.pos.clone());
            let name_idx = chunk.add_constant(str_obj(ip.name.clone()));
            chunk.write_op(Opcode::StoreName, ip.pos.clone());
            chunk.write_u16(name_idx, ip.pos.clone());
        }
        if !arm.binding_name.is_empty() {
            emit_load_name(chunk, &subject_name, arm.binding_pos.clone());
            let name_idx = chunk.add_constant(str_obj(arm.binding_name.clone()));
            chunk.write_op(Opcode::StoreName, arm.binding_pos.clone());
            chunk.write_u16(name_idx, arm.binding_pos.clone());
        }
        if let Some(guard) = &arm.guard {
            compile_expr(guard, chunk, resolutions)?;
            let guard_failed = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, guard.pos());
            compile_match_body(&arm.body, chunk, resolutions, compile_expr, compile_stmt)?;
            to_end.push(emit_jump_placeholder(chunk, Opcode::Jump, arm.pos.clone()));
            patch_jump_here(chunk, guard_failed);
        } else {
            compile_match_body(&arm.body, chunk, resolutions, compile_expr, compile_stmt)?;
            to_end.push(emit_jump_placeholder(chunk, Opcode::Jump, arm.pos.clone()));
        }
        patch_jump_here(chunk, to_next);
    }

    emit_load_name(chunk, &subject_name, m.pos.clone());
    chunk.write_op(Opcode::ThrowMatchError, m.pos.clone());
    for patch in to_end {
        patch_jump_here(chunk, patch);
    }
    Ok(())
}

fn compile_match_body(
    body: &MatchBody,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
    compile_stmt: CompileStmtFn,
) -> Result<(), Object> {
    match body {
        MatchBody::Expr(expr) => compile_expr(expr, chunk, resolutions),
        MatchBody::Block(block) => {
            let n = block.statements.len();
            if n == 0 {
                let idx = chunk.add_constant(Object::Undefined);
                emit_const(chunk, idx, block.pos.clone());
                return Ok(());
            }
            for (i, stmt) in block.statements.iter().enumerate() {
                compile_stmt(
                    stmt,
                    chunk,
                    &mut Vec::new(),
                    &mut Vec::new(),
                    i + 1 == n,
                    resolutions,
                )?;
            }
            Ok(())
        }
    }
}

fn compile_pattern_test(
    pattern: &Pattern,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    match pattern {
        Pattern::Literal(lp) => {
            compile_expr(&lp.value, chunk, resolutions)?;
            chunk.write_op(Opcode::Eq, lp.pos.clone());
            Ok(())
        }
        Pattern::Ident(_) | Pattern::Wildcard(_) => {
            chunk.write_op(Opcode::Pop, pattern_pos(pattern));
            let idx = chunk.add_constant(bool_obj(true));
            emit_const(chunk, idx, pattern_pos(pattern));
            Ok(())
        }
        Pattern::Or(op) => {
            let mut to_true = Vec::new();
            for alt in &op.alternatives {
                chunk.write_op(Opcode::Dup, pattern_pos(alt));
                compile_pattern_test(alt, chunk, resolutions, compile_expr)?;
                to_true.push(emit_jump_placeholder(
                    chunk,
                    Opcode::JumpIfTrue,
                    pattern_pos(alt),
                ));
            }
            chunk.write_op(Opcode::Pop, op.pos.clone());
            let false_idx = chunk.add_constant(bool_obj(false));
            emit_const(chunk, false_idx, op.pos.clone());
            let to_end = emit_jump_placeholder(chunk, Opcode::Jump, op.pos.clone());
            for patch in to_true {
                patch_jump_here(chunk, patch);
            }
            chunk.write_op(Opcode::Pop, op.pos.clone());
            let true_idx = chunk.add_constant(bool_obj(true));
            emit_const(chunk, true_idx, op.pos.clone());
            patch_jump_here(chunk, to_end);
            Ok(())
        }
        Pattern::Range(rp) => {
            chunk.write_op(Opcode::Dup, rp.pos.clone());
            compile_expr(&rp.start, chunk, resolutions)?;
            chunk.write_op(Opcode::Ge, rp.pos.clone());
            let to_false = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, rp.pos.clone());
            compile_expr(&rp.end, chunk, resolutions)?;
            if rp.inclusive {
                chunk.write_op(Opcode::Le, rp.pos.clone());
            } else {
                chunk.write_op(Opcode::Lt, rp.pos.clone());
            }
            let to_end = emit_jump_placeholder(chunk, Opcode::Jump, rp.pos.clone());
            patch_jump_here(chunk, to_false);
            chunk.write_op(Opcode::Pop, rp.pos.clone());
            let false_idx = chunk.add_constant(bool_obj(false));
            emit_const(chunk, false_idx, rp.pos.clone());
            patch_jump_here(chunk, to_end);
            Ok(())
        }
    }
}

fn pattern_pos(pattern: &Pattern) -> crate::ast::Position {
    match pattern {
        Pattern::Literal(p) => p.pos.clone(),
        Pattern::Ident(p) => p.pos.clone(),
        Pattern::Wildcard(p) => p.pos.clone(),
        Pattern::Or(p) => p.pos.clone(),
        Pattern::Range(p) => p.pos.clone(),
    }
}
