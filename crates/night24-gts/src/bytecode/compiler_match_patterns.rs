use crate::ast::{Expr, Pattern, Position};
use crate::object::{bool_obj, Object};

use super::chunk::Chunk;
use super::emit::{emit_const, emit_jump_placeholder, patch_jump_here};
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

type CompileExprFn = fn(&Expr, &mut Chunk, &ResolutionMap) -> Result<(), Object>;

pub(super) fn compile_pattern_test(
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

fn pattern_pos(pattern: &Pattern) -> Position {
    match pattern {
        Pattern::Literal(p) => p.pos.clone(),
        Pattern::Ident(p) => p.pos.clone(),
        Pattern::Wildcard(p) => p.pos.clone(),
        Pattern::Or(p) => p.pos.clone(),
        Pattern::Range(p) => p.pos.clone(),
    }
}
