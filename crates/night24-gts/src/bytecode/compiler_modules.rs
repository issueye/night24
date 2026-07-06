use crate::ast::{ExportDecl, ExportSpec, ImportDecl, Position, Stmt};
use crate::object::Object;

use super::chunk::Chunk;
use super::compiler::{compile_expr, compile_stmt, FinallyFrame, LoopFrame};
use super::emit::emit_load_name;
use super::emit::emit_string_operand;
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

pub(super) fn compile_import(s: &ImportDecl, chunk: &mut Chunk) -> Result<(), Object> {
    let source = crate::evaluator::eval_core::strip_quotes(&s.source);
    emit_string_operand(chunk, Opcode::ImportModule, source, s.pos.clone());

    if !s.default.is_empty() {
        compile_import_binding("default", &s.default, s.pos.clone(), chunk);
    }
    if !s.namespace.is_empty() {
        chunk.write_op(Opcode::Dup, s.pos.clone());
        emit_string_operand(chunk, Opcode::StoreName, s.namespace.clone(), s.pos.clone());
    }
    for name in &s.names {
        compile_import_binding(name, name, s.pos.clone(), chunk);
    }
    let mut aliases: Vec<_> = s.aliases.iter().collect();
    aliases.sort_by_key(|(left, _)| *left);
    for (exported, local) in aliases {
        compile_import_binding(exported, local, s.pos.clone(), chunk);
    }

    chunk.write_op(Opcode::Pop, s.pos.clone());
    Ok(())
}

pub(super) fn compile_export(
    s: &ExportDecl,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    finalizers: &mut Vec<FinallyFrame>,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    if s.is_star {
        // `export * from "..."` aggregates every named export from the source.
        let source = crate::evaluator::eval_core::strip_quotes(&s.from);
        emit_string_operand(chunk, Opcode::ImportModule, source, s.pos.clone());
        chunk.write_op(Opcode::ExportAll, s.pos.clone());
        return Ok(());
    }

    if !s.from.is_empty() {
        let source = crate::evaluator::eval_core::strip_quotes(&s.from);
        emit_string_operand(chunk, Opcode::ImportModule, source, s.pos.clone());
        for spec in &s.specifiers {
            compile_reexport_spec(spec, s.pos.clone(), chunk);
        }
        chunk.write_op(Opcode::Pop, s.pos.clone());
        return Ok(());
    }

    if let Some(decl) = &s.decl {
        if s.is_default {
            if let Stmt::Expr(expr_stmt) = decl.as_ref() {
                compile_expr(&expr_stmt.expr, chunk, resolutions)?;
                compile_export_stack_value("default", s.pos.clone(), chunk);
            } else {
                compile_stmt(decl, chunk, loops, finalizers, false, resolutions)?;
            }
        } else {
            compile_stmt(decl, chunk, loops, finalizers, false, resolutions)?;
            if let Some(name) = exported_decl_name(decl) {
                compile_export_local_name(&name, &name, s.pos.clone(), chunk);
            }
        }
    }

    for spec in &s.specifiers {
        compile_export_local_name(&spec.name, &spec.alias, s.pos.clone(), chunk);
    }
    Ok(())
}

fn compile_import_binding(exported_name: &str, local_name: &str, pos: Position, chunk: &mut Chunk) {
    chunk.write_op(Opcode::Dup, pos.clone());
    emit_string_operand(chunk, Opcode::GetProperty, exported_name, pos.clone());
    emit_string_operand(chunk, Opcode::StoreName, local_name, pos);
}

fn exported_decl_name(stmt: &Stmt) -> Option<String> {
    match stmt {
        Stmt::FuncDecl(f) => Some(f.name.clone()),
        Stmt::ClassDecl(c) => Some(c.name.clone()),
        Stmt::Let(l) => Some(l.name.clone()),
        Stmt::Const(c) => Some(c.name.clone()),
        Stmt::Var(v) => Some(v.name.clone()),
        _ => None,
    }
}

fn compile_reexport_spec(spec: &ExportSpec, pos: Position, chunk: &mut Chunk) {
    chunk.write_op(Opcode::Dup, pos.clone());
    emit_string_operand(chunk, Opcode::GetProperty, spec.name.clone(), pos.clone());
    compile_export_stack_value(&spec.alias, pos, chunk);
}

fn compile_export_local_name(
    local_name: &str,
    exported_name: &str,
    pos: Position,
    chunk: &mut Chunk,
) {
    emit_load_name(chunk, local_name, pos.clone());
    compile_export_stack_value(exported_name, pos, chunk);
}

fn compile_export_stack_value(exported_name: &str, pos: Position, chunk: &mut Chunk) {
    emit_string_operand(chunk, Opcode::ExportName, exported_name, pos);
}
