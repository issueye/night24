use crate::ast::{ExportDecl, ExportSpec, ImportDecl, Position, Stmt};
use crate::object::{str_obj, Object};

use super::chunk::Chunk;
use super::compiler::{compile_expr, compile_stmt, FinallyFrame, LoopFrame};
use super::emit::emit_load_name;
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

pub(super) fn compile_import(s: &ImportDecl, chunk: &mut Chunk) -> Result<(), Object> {
    let source = crate::evaluator::eval_core::strip_quotes(&s.source);
    let source_idx = chunk.add_constant(str_obj(source));
    chunk.write_op(Opcode::ImportModule, s.pos.clone());
    chunk.write_u16(source_idx, s.pos.clone());

    if !s.default.is_empty() {
        compile_import_binding("default", &s.default, s.pos.clone(), chunk);
    }
    if !s.namespace.is_empty() {
        chunk.write_op(Opcode::Dup, s.pos.clone());
        let name_idx = chunk.add_constant(str_obj(s.namespace.clone()));
        chunk.write_op(Opcode::StoreName, s.pos.clone());
        chunk.write_u16(name_idx, s.pos.clone());
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
        let source_idx = chunk.add_constant(str_obj(source));
        chunk.write_op(Opcode::ImportModule, s.pos.clone());
        chunk.write_u16(source_idx, s.pos.clone());
        chunk.write_op(Opcode::ExportAll, s.pos.clone());
        return Ok(());
    }

    if !s.from.is_empty() {
        let source = crate::evaluator::eval_core::strip_quotes(&s.from);
        let source_idx = chunk.add_constant(str_obj(source));
        chunk.write_op(Opcode::ImportModule, s.pos.clone());
        chunk.write_u16(source_idx, s.pos.clone());
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
    let property_idx = chunk.add_constant(str_obj(exported_name));
    chunk.write_op(Opcode::GetProperty, pos.clone());
    chunk.write_u16(property_idx, pos.clone());
    let local_idx = chunk.add_constant(str_obj(local_name));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(local_idx, pos);
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
    let property_idx = chunk.add_constant(str_obj(spec.name.clone()));
    chunk.write_op(Opcode::GetProperty, pos.clone());
    chunk.write_u16(property_idx, pos.clone());
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
    let exported_idx = chunk.add_constant(str_obj(exported_name));
    chunk.write_op(Opcode::ExportName, pos.clone());
    chunk.write_u16(exported_idx, pos);
}
