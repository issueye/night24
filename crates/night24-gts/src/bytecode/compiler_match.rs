use crate::ast::{Expr, MatchBody, MatchExpr, Pattern, Stmt};
use crate::object::Object;

use super::chunk::Chunk;
use super::compiler::{FinallyFrame, LoopFrame};
use super::compiler_match_patterns::compile_pattern_test;
use super::emit::emit_string_operand;
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
    emit_string_operand(
        chunk,
        Opcode::StoreName,
        subject_name.clone(),
        m.pos.clone(),
    );

    let mut to_end = Vec::new();
    for arm in &m.arms {
        emit_load_name(chunk, &subject_name, arm.pos.clone());
        compile_pattern_test(&arm.pattern, chunk, resolutions, compile_expr)?;
        let to_next = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, arm.pos.clone());

        if let Pattern::Ident(ip) = &arm.pattern {
            emit_load_name(chunk, &subject_name, ip.pos.clone());
            emit_string_operand(chunk, Opcode::StoreName, ip.name.clone(), ip.pos.clone());
        }
        if !arm.binding_name.is_empty() {
            emit_load_name(chunk, &subject_name, arm.binding_pos.clone());
            emit_string_operand(
                chunk,
                Opcode::StoreName,
                arm.binding_name.clone(),
                arm.binding_pos.clone(),
            );
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
