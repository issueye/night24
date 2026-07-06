use crate::ast::Stmt;
use crate::object::Object;

use super::chunk::Chunk;
use super::compiler_abrupt::{
    compile_break_continue, compile_return_stmt, FinallyFrame, LoopFrame,
};
use super::compiler_classes::compile_class_decl;
use super::compiler_control::{compile_for, compile_if, compile_labeled, compile_while};
use super::compiler_declarations::{compile_const_stmt, compile_let_stmt, compile_var_stmt};
use super::compiler_expr::compile_expr;
use super::compiler_functions::compile_func_decl;
use super::compiler_iterators::{compile_for_in, compile_for_of};
use super::compiler_modules::{compile_export, compile_import};
use super::compiler_try::compile_try;
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

pub(super) fn compile_stmt(
    stmt: &Stmt,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    finalizers: &mut Vec<FinallyFrame>,
    keep_value: bool,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    match stmt {
        Stmt::Expr(e) => {
            compile_expr(&e.expr, chunk, resolutions)?;
            if !keep_value {
                // Discard the expression value so it doesn't accumulate on the
                // stack across iterations / statements. The top-level last
                // statement keeps its value as the program result.
                chunk.write_op(Opcode::Pop, e.pos.clone());
            }
            Ok(())
        }
        Stmt::Let(s) => compile_let_stmt(s, chunk, resolutions),
        Stmt::Var(s) => compile_var_stmt(s, chunk, resolutions),
        Stmt::Const(s) => compile_const_stmt(s, chunk, resolutions),
        Stmt::Block(b) => {
            for s in &b.statements {
                compile_stmt(s, chunk, loops, finalizers, false, resolutions)?;
            }
            Ok(())
        }
        Stmt::If(s) => compile_if(s, chunk, loops, finalizers, keep_value, resolutions),
        Stmt::While(s) => compile_while(s, None, chunk, loops, finalizers, keep_value, resolutions),
        Stmt::For(s) => compile_for(s, None, chunk, loops, finalizers, keep_value, resolutions),
        Stmt::ForIn(s) => compile_for_in(
            &s.name,
            &s.iterable,
            &s.body,
            s.pos.clone(),
            None,
            chunk,
            loops,
            finalizers,
            resolutions,
        ),
        Stmt::ForOf(s) => compile_for_of(
            &s.name,
            &s.iterable,
            &s.body,
            s.pos.clone(),
            None,
            chunk,
            loops,
            finalizers,
            resolutions,
        ),
        Stmt::Break(s) => {
            compile_break_continue(true, &s.label, s.pos.clone(), chunk, loops, finalizers)
        }
        Stmt::Continue(s) => {
            compile_break_continue(false, &s.label, s.pos.clone(), chunk, loops, finalizers)
        }
        Stmt::Labeled(s) => compile_labeled(s, chunk, loops, finalizers, keep_value, resolutions),
        Stmt::Throw(s) => {
            compile_expr(&s.value, chunk, resolutions)?;
            chunk.write_op(Opcode::Throw, s.pos.clone());
            Ok(())
        }
        Stmt::Try(s) => compile_try(s, chunk, loops, finalizers, resolutions),
        Stmt::Import(s) => compile_import(s, chunk),
        Stmt::Export(s) => compile_export(s, chunk, loops, finalizers, resolutions),
        Stmt::FuncDecl(f) => compile_func_decl(f, chunk, resolutions),
        Stmt::ClassDecl(c) => compile_class_decl(c, chunk),
        Stmt::Return(r) => compile_return_stmt(r, chunk, loops, finalizers, resolutions),
    }
}
