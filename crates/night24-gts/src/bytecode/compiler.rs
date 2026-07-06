//! The compiler: walks an AST once and emits a `Chunk`.
//!
//! Stage 0 coverage (kept deliberately minimal — see
//! `docs/bytecode-vm-development-plan.md` §3.5):
//!   - `Stmt::Expr` wrapping an expression statement
//!   - `Expr::Number`           → CONST
//!   - `Expr::Infix` with op `+` → post-order: left, right, ADD
//!   - trailing RETURN for the top-level program
//!
//! Every other AST node returns a compile error rather than emitting broken
//! bytecode. This is by design: a stage-N PR must extend coverage and remove
//! the corresponding error path; nothing compiles to "do nothing".

use crate::ast::Program;
use crate::object::{new_error, Object};

use super::chunk::Chunk;
pub(super) use super::compiler_abrupt::{compile_break_continue, FinallyFrame, LoopFrame};
pub(super) use super::compiler_expr::compile_expr;
pub(super) use super::compiler_functions::compile_method_proto;
pub(super) use super::compiler_stmt::compile_stmt;
use super::opcode::Opcode;
use super::resolve;

/// Compile a whole program. Emits each statement in order followed by a
/// terminal RETURN, so the interpreter leaves the last value on the stack.
///
/// `resolutions` is threaded through the compile functions for a single
/// purpose: at function-prototype construction (`compile_method_proto`,
/// ~line 913) it supplies each function's upvalue capture descriptors.
/// The emit path itself does NOT consult `resolutions` — variable access is
/// still lowered unconditionally to `LoadName`/`StoreName`. (Enabling the
/// local/global fast paths requires resolving a storage-model mismatch; see
/// `docs/local-slot-optimization-plan.md`.) Keeping the parameter avoids a
/// separate resolver pass per function.
pub fn compile(program: &Program) -> Result<Chunk, Object> {
    let resolutions = resolve::resolve_program(program);
    let mut chunk = Chunk::new();
    let mut loops: Vec<LoopFrame> = Vec::new();
    let mut finalizers: Vec<FinallyFrame> = Vec::new();
    let n = program.body.len();
    for (i, stmt) in program.body.iter().enumerate() {
        compile_stmt(
            stmt,
            &mut chunk,
            &mut loops,
            &mut finalizers,
            i + 1 == n,
            &resolutions,
        )?;
    }
    // Top-level RETURN: the program's result is whatever sits on the stack.
    chunk.write_op(Opcode::Return, program.pos.clone());
    Ok(chunk)
}

pub(super) fn unsupported(pos: crate::ast::Position, what: &str) -> Object {
    new_error(
        pos,
        format!("CompileError: bytecode VM does not yet support {}", what),
    )
}

#[cfg(test)]
mod tests;
