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

use crate::ast::{Expr, MatchBody, Pattern, Program, Stmt};
use crate::evaluator::string_lit::{eval_regexp_lit, eval_string_lit};
use crate::lexer::Lexer;
use crate::object::{bool_obj, new_error, num_obj, str_obj, Object};
use crate::parser::Parser;
use std::rc::Rc;

use super::chunk::Chunk;
use super::closure::FunctionProto;
use super::opcode::Opcode;
use super::resolve::{self, ResolutionMap};

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
    let n = program.body.len();
    for (i, stmt) in program.body.iter().enumerate() {
        compile_stmt(stmt, &mut chunk, &mut loops, i + 1 == n, &resolutions)?;
    }
    // Top-level RETURN: the program's result is whatever sits on the stack.
    chunk.write_op(Opcode::Return, program.pos.clone());
    Ok(chunk)
}

/// A loop being compiled: holds patch sites for `break` (jump to end) and
/// `continue` (jump to the post-expression / condition re-test).
#[derive(Default)]
struct LoopFrame {
    /// Optional label attached to this loop.
    label: Option<String>,
    /// Byte offsets of pending `break` jumps (each is a JUMP placeholder).
    breaks: Vec<u32>,
    /// Byte offsets of pending `continue` jumps.
    continues: Vec<u32>,
}

fn compile_stmt(
    stmt: &Stmt,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
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
        Stmt::Let(s) => {
            if let Some(binding) = &s.binding {
                return compile_destructure(
                    binding,
                    s.value.as_ref(),
                    s.pos.clone(),
                    false,
                    chunk,
                    resolutions,
                );
            }
            compile_decl(
                &s.name,
                s.value.as_ref(),
                s.type_anno.as_ref(),
                false,
                s.pos.clone(),
                chunk,
                resolutions,
            )
        }
        Stmt::Var(s) => {
            if let Some(binding) = &s.binding {
                return compile_destructure(
                    binding,
                    s.value.as_ref(),
                    s.pos.clone(),
                    false,
                    chunk,
                    resolutions,
                );
            }
            compile_decl(
                &s.name,
                s.value.as_ref(),
                s.type_anno.as_ref(),
                false,
                s.pos.clone(),
                chunk,
                resolutions,
            )
        }
        Stmt::Const(s) => {
            if let Some(binding) = &s.binding {
                return compile_destructure(
                    binding,
                    s.value.as_ref(),
                    s.pos.clone(),
                    true,
                    chunk,
                    resolutions,
                );
            }
            compile_decl(
                &s.name,
                s.value.as_ref(),
                s.type_anno.as_ref(),
                true,
                s.pos.clone(),
                chunk,
                resolutions,
            )
        }
        Stmt::Block(b) => {
            for s in &b.statements {
                compile_stmt(s, chunk, loops, false, resolutions)?;
            }
            Ok(())
        }
        Stmt::If(s) => compile_if(s, chunk, loops, keep_value, resolutions),
        Stmt::While(s) => compile_while(s, None, chunk, loops, keep_value, resolutions),
        Stmt::For(s) => compile_for(s, None, chunk, loops, keep_value, resolutions),
        Stmt::ForIn(s) => compile_for_in(
            &s.name,
            &s.iterable,
            &s.body,
            s.pos.clone(),
            None,
            chunk,
            loops,
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
            resolutions,
        ),
        Stmt::Break(s) => compile_break_continue(true, &s.label, s.pos.clone(), chunk, loops),
        Stmt::Continue(s) => compile_break_continue(false, &s.label, s.pos.clone(), chunk, loops),
        Stmt::Labeled(s) => compile_labeled(s, chunk, loops, keep_value, resolutions),
        Stmt::Throw(s) => {
            compile_expr(&s.value, chunk, resolutions)?;
            chunk.write_op(Opcode::Throw, s.pos.clone());
            Ok(())
        }
        Stmt::Try(s) => compile_try(s, chunk, loops, resolutions),
        Stmt::Import(s) => compile_import(s, chunk),
        Stmt::Export(s) => compile_export(s, chunk, loops, resolutions),
        Stmt::FuncDecl(f) => {
            // Compile the body to a proto (which lives in this chunk's proto
            // table), emit OP_CLOSURE to construct the closure value, then
            // store it under the function's name.
            let proto_idx = compile_function_proto(
                &f.name,
                f.params.clone(),
                f.body.clone(),
                f.is_async,
                false,
                f.return_t.clone(),
                f.pos.clone(),
                chunk,
                resolutions,
            )?;
            chunk.write_op(Opcode::Closure, f.pos.clone());
            chunk.write_u16(proto_idx, f.pos.clone());
            let name_idx = chunk.add_constant(str_obj(f.name.clone()));
            chunk.write_op(Opcode::StoreName, f.pos.clone());
            chunk.write_u16(name_idx, f.pos.clone());
            Ok(())
        }
        Stmt::ClassDecl(c) => {
            let class_idx = add_class_decl(chunk, c.clone());
            chunk.write_op(Opcode::NewClass, c.pos.clone());
            chunk.write_u16(class_idx, c.pos.clone());
            let name_idx = chunk.add_constant(str_obj(c.name.clone()));
            chunk.write_op(Opcode::StoreName, c.pos.clone());
            chunk.write_u16(name_idx, c.pos.clone());
            Ok(())
        }
        Stmt::Return(r) => {
            if let Some(v) = &r.value {
                compile_expr(v, chunk, resolutions)?;
            } else {
                let idx = chunk.add_constant(Object::Undefined);
                emit_const(chunk, idx, r.pos.clone());
            }
            chunk.write_op(Opcode::Return, r.pos.clone());
            Ok(())
        }
    }
}

fn compile_try(
    s: &crate::ast::TryStmt,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    let try_start = chunk.code.len() as u32;
    for stmt in &s.block.statements {
        compile_stmt(stmt, chunk, loops, false, resolutions)?;
    }
    let try_end = chunk.code.len() as u32;
    let to_normal_finally = emit_jump_placeholder(chunk, Opcode::Jump, s.pos.clone());

    let catch_start = chunk.code.len() as u32;
    let mut catch_end = catch_start;
    if let Some(catch) = &s.catch {
        if catch.name.is_empty() {
            chunk.write_op(Opcode::Pop, catch.pos.clone());
        } else {
            let name_idx = chunk.add_constant(str_obj(catch.name.clone()));
            chunk.write_op(Opcode::StoreName, catch.pos.clone());
            chunk.write_u16(name_idx, catch.pos.clone());
        }
        for stmt in &catch.body.statements {
            compile_stmt(stmt, chunk, loops, false, resolutions)?;
        }
        catch_end = chunk.code.len() as u32;
    }

    patch_jump_here(chunk, to_normal_finally);
    if let Some(finalizer) = &s.finalizer {
        for stmt in &finalizer.statements {
            compile_stmt(stmt, chunk, loops, false, resolutions)?;
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
        let pending_idx = chunk.add_constant(str_obj(pending_name.clone()));
        chunk.write_op(Opcode::StoreName, s.pos.clone());
        chunk.write_u16(pending_idx, s.pos.clone());
        for stmt in &finalizer.statements {
            compile_stmt(stmt, chunk, loops, false, resolutions)?;
        }
        emit_load_name(chunk, &pending_name, s.pos.clone());
        chunk.write_op(Opcode::Throw, s.pos.clone());
    }

    if let Some(end) = to_end {
        patch_jump_here(chunk, end);
    }

    let handler_ip = if s.catch.is_some() {
        catch_start
    } else {
        exceptional_finally_ip.unwrap_or(catch_start)
    };
    chunk.protected_regions.push(super::chunk::ProtectedRegion {
        try_start,
        try_end,
        handler_ip,
        finally_ip: exceptional_finally_ip,
        catch_binding_slot: None,
    });
    if s.finalizer.is_some() && catch_end > catch_start {
        chunk.protected_regions.push(super::chunk::ProtectedRegion {
            try_start: catch_start,
            try_end: catch_end,
            handler_ip: exceptional_finally_ip.unwrap(),
            finally_ip: exceptional_finally_ip,
            catch_binding_slot: None,
        });
    }
    Ok(())
}

fn compile_import(s: &crate::ast::ImportDecl, chunk: &mut Chunk) -> Result<(), Object> {
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

fn compile_import_binding(
    exported_name: &str,
    local_name: &str,
    pos: crate::ast::Position,
    chunk: &mut Chunk,
) {
    chunk.write_op(Opcode::Dup, pos.clone());
    let property_idx = chunk.add_constant(str_obj(exported_name));
    chunk.write_op(Opcode::GetProperty, pos.clone());
    chunk.write_u16(property_idx, pos.clone());
    let local_idx = chunk.add_constant(str_obj(local_name));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(local_idx, pos);
}

fn compile_export(
    s: &crate::ast::ExportDecl,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    if s.is_star {
        // `export * from "..."` — aggregate every named export from the source.
        // ImportModule pushes the source's exports object; ExportAll copies all
        // its properties into the current module's exports (skipping `default`).
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
                compile_stmt(decl, chunk, loops, false, resolutions)?;
            }
        } else {
            compile_stmt(decl, chunk, loops, false, resolutions)?;
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

fn compile_reexport_spec(
    spec: &crate::ast::ExportSpec,
    pos: crate::ast::Position,
    chunk: &mut Chunk,
) {
    chunk.write_op(Opcode::Dup, pos.clone());
    let property_idx = chunk.add_constant(str_obj(spec.name.clone()));
    chunk.write_op(Opcode::GetProperty, pos.clone());
    chunk.write_u16(property_idx, pos.clone());
    compile_export_stack_value(&spec.alias, pos, chunk);
}

fn compile_export_local_name(
    local_name: &str,
    exported_name: &str,
    pos: crate::ast::Position,
    chunk: &mut Chunk,
) {
    emit_load_name(chunk, local_name, pos.clone());
    compile_export_stack_value(exported_name, pos, chunk);
}

fn compile_export_stack_value(exported_name: &str, pos: crate::ast::Position, chunk: &mut Chunk) {
    let exported_idx = chunk.add_constant(str_obj(exported_name));
    chunk.write_op(Opcode::ExportName, pos.clone());
    chunk.write_u16(exported_idx, pos);
}

/// Compile `if (cond) { ... } else { ... }`.
fn compile_if(
    s: &crate::ast::IfStmt,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    _keep_value: bool,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    // cond ; JUMP_IF_FALSE else ; <then> ; JUMP end ; else: <else> ; end:
    compile_expr(&s.cond, chunk, resolutions)?;
    let to_else = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, s.pos.clone());
    for stmt in &s.consequence.statements {
        compile_stmt(stmt, chunk, loops, false, resolutions)?;
    }
    let to_end = if s.alternative.is_some() {
        Some(emit_jump_placeholder(chunk, Opcode::Jump, s.pos.clone()))
    } else {
        None
    };
    patch_jump_here(chunk, to_else);
    if let Some(alt) = &s.alternative {
        compile_stmt(alt, chunk, loops, false, resolutions)?;
    }
    if let Some(end) = to_end {
        patch_jump_here(chunk, end);
    }
    Ok(())
}

/// Compile `while (cond) { body }`.
fn compile_while(
    s: &crate::ast::WhileStmt,
    label: Option<String>,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    _keep_value: bool,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    // start: cond ; JUMP_IF_FALSE end ; <body> ; LOOP start ; end:
    let start = chunk.code.len() as u32;
    compile_expr(&s.cond, chunk, resolutions)?;
    let to_end = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, s.pos.clone());
    loops.push(LoopFrame {
        label,
        ..LoopFrame::default()
    });
    for stmt in &s.body.statements {
        compile_stmt(stmt, chunk, loops, false, resolutions)?;
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
fn compile_for(
    s: &crate::ast::ForStmt,
    label: Option<String>,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    _keep_value: bool,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    // <init> ; start: <cond> ; JUMP_IF_FALSE end ; <body> ; post_start: <post> ; LOOP start ; end:
    if let Some(init) = &s.init {
        compile_stmt(init, chunk, loops, false, resolutions)?;
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
    loops.push(LoopFrame {
        label,
        ..LoopFrame::default()
    });
    for stmt in &s.body.statements {
        compile_stmt(stmt, chunk, loops, false, resolutions)?;
    }
    let frame = loops.pop().unwrap();
    // post expression (continue targets here) — recorded AFTER the body so its
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

#[allow(clippy::too_many_arguments)]
fn compile_for_in(
    name: &str,
    iterable: &Expr,
    body: &crate::ast::BlockStmt,
    pos: crate::ast::Position,
    label: Option<String>,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    let suffix = format!("{}_{}", pos.line, pos.col);
    let items_name = format!("__gts_bc_iter_items_{}", suffix);
    let idx_name = format!("__gts_bc_iter_idx_{}", suffix);

    // items = ITER_KEYS/ITER_VALUES(iterable)
    compile_expr(iterable, chunk, resolutions)?;
    chunk.write_op(Opcode::IterKeys, pos.clone());
    let items_idx = chunk.add_constant(str_obj(items_name.clone()));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(items_idx, pos.clone());

    // idx = 0
    let zero = chunk.add_constant(num_obj(0.0));
    emit_const(chunk, zero, pos.clone());
    let idx_idx = chunk.add_constant(str_obj(idx_name.clone()));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(idx_idx, pos.clone());

    // start: idx < len(items)
    let start = chunk.code.len() as u32;
    emit_load_name(chunk, &idx_name, pos.clone());
    emit_load_name(chunk, &items_name, pos.clone());
    chunk.write_op(Opcode::Len, pos.clone());
    chunk.write_op(Opcode::Lt, pos.clone());
    let to_end = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, pos.clone());

    // loop variable = items[idx]
    emit_load_name(chunk, &items_name, pos.clone());
    emit_load_name(chunk, &idx_name, pos.clone());
    chunk.write_op(Opcode::GetIndex, pos.clone());
    let name_idx = chunk.add_constant(str_obj(name.to_string()));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(name_idx, pos.clone());

    loops.push(LoopFrame {
        label,
        ..LoopFrame::default()
    });
    for stmt in &body.statements {
        compile_stmt(stmt, chunk, loops, false, resolutions)?;
    }
    let frame = loops.pop().unwrap();

    // continue target: idx = idx + 1
    let increment = chunk.code.len() as u32;
    emit_load_name(chunk, &idx_name, pos.clone());
    let one = chunk.add_constant(num_obj(1.0));
    emit_const(chunk, one, pos.clone());
    chunk.write_op(Opcode::Add, pos.clone());
    chunk.write_op(Opcode::Dup, pos.clone());
    let idx_idx = chunk.add_constant(str_obj(idx_name));
    chunk.write_op(Opcode::AssignName, pos.clone());
    chunk.write_u16(idx_idx, pos.clone());
    chunk.write_op(Opcode::Pop, pos.clone());
    chunk.write_op(Opcode::Loop, pos.clone());
    chunk.write_u32(start, pos.clone());

    let end = chunk.code.len() as u32;
    patch_jump_here(chunk, to_end);
    for b in &frame.breaks {
        patch_jump_to(chunk, *b, end);
    }
    for c in &frame.continues {
        patch_jump_to(chunk, *c, increment);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn compile_for_of(
    name: &str,
    iterable: &Expr,
    body: &crate::ast::BlockStmt,
    pos: crate::ast::Position,
    label: Option<String>,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    let suffix = format!("{}_{}", pos.line, pos.col);
    let iter_name = format!("__gts_bc_iter_{}", suffix);
    let next_name = format!("__gts_bc_iter_next_{}", suffix);

    compile_expr(iterable, chunk, resolutions)?;
    chunk.write_op(Opcode::IterValues, pos.clone());
    let iter_idx = chunk.add_constant(str_obj(iter_name.clone()));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(iter_idx, pos.clone());

    let start = chunk.code.len() as u32;
    emit_load_name(chunk, &iter_name, pos.clone());
    chunk.write_op(Opcode::IterNext, pos.clone());
    let next_idx = chunk.add_constant(str_obj(next_name.clone()));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(next_idx, pos.clone());

    emit_load_name(chunk, &next_name, pos.clone());
    let done_idx = chunk.add_constant(str_obj("done"));
    chunk.write_op(Opcode::GetProperty, pos.clone());
    chunk.write_u16(done_idx, pos.clone());
    let to_end = emit_jump_placeholder(chunk, Opcode::JumpIfTrue, pos.clone());

    emit_load_name(chunk, &next_name, pos.clone());
    let value_idx = chunk.add_constant(str_obj("value"));
    chunk.write_op(Opcode::GetProperty, pos.clone());
    chunk.write_u16(value_idx, pos.clone());
    let name_idx = chunk.add_constant(str_obj(name.to_string()));
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(name_idx, pos.clone());

    loops.push(LoopFrame {
        label,
        ..LoopFrame::default()
    });
    for stmt in &body.statements {
        compile_stmt(stmt, chunk, loops, false, resolutions)?;
    }
    let frame = loops.pop().unwrap();

    chunk.write_op(Opcode::Loop, pos.clone());
    chunk.write_u32(start, pos.clone());

    let end = chunk.code.len() as u32;
    patch_jump_here(chunk, to_end);
    for b in &frame.breaks {
        patch_jump_to(chunk, *b, end);
    }
    for c in &frame.continues {
        patch_jump_to(chunk, *c, start);
    }
    Ok(())
}

fn compile_labeled(
    s: &crate::ast::LabeledStmt,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
    keep_value: bool,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    match s.stmt.as_ref() {
        Stmt::While(w) => compile_while(
            w,
            Some(s.label.clone()),
            chunk,
            loops,
            keep_value,
            resolutions,
        ),
        Stmt::For(f) => compile_for(
            f,
            Some(s.label.clone()),
            chunk,
            loops,
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
            resolutions,
        ),
        other => compile_stmt(other, chunk, loops, keep_value, resolutions),
    }
}

/// Compile `break` (is_break=true) or `continue`. Records a pending JUMP in
/// the current loop frame to be patched when the loop's end / continue-target
/// is known. Labeled break/continue is stage 2 polish (defers to plain).
#[allow(clippy::ptr_arg)]
fn compile_break_continue(
    is_break: bool,
    label: &str,
    pos: crate::ast::Position,
    chunk: &mut Chunk,
    loops: &mut Vec<LoopFrame>,
) -> Result<(), Object> {
    let frame = match loops.iter_mut().rev().find(|f| {
        label.is_empty()
            || f.label
                .as_ref()
                .map(|frame_label| frame_label == label)
                .unwrap_or(false)
    }) {
        Some(f) => f,
        None => {
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
        }
    };
    let patch = emit_jump_placeholder(chunk, Opcode::Jump, pos);
    if is_break {
        frame.breaks.push(patch);
    } else {
        frame.continues.push(patch);
    }
    Ok(())
}

/// Compile an interpolated template literal into a string concatenation.
///
/// Each `${expr}` segment is re-parsed as a sub-expression (matching the
/// tree-walker's `eval_template_expression`), evaluated, and converted to its
/// string form via TO_STRING. Literal text segments are CONST strings. All
/// parts are joined left-to-right with `+` (string concat).
fn compile_template_interpolated(
    t: &crate::ast::TemplateLit,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    let lit = &t.literal;
    if lit.len() < 2 || !lit.starts_with('`') {
        let value = crate::evaluator::string_lit::eval_template_static(t);
        let idx = chunk.add_constant(value);
        emit_const(chunk, idx, t.pos.clone());
        return Ok(());
    }
    let mut inner = &lit[1..];
    if inner.ends_with('`') {
        inner = &inner[..inner.len() - 1];
    }
    let bytes = inner.as_bytes();
    let mut segments_emitted = 0;
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            let end = match find_template_expr_end(inner, i + 2) {
                Some(end) => end,
                None => {
                    return Err(unsupported(
                        t.pos.clone(),
                        "unterminated template expression",
                    ));
                }
            };
            let expr_str = inner[i + 2..end].trim();
            if !expr_str.is_empty() {
                // Re-parse the sub-expression at compile time so the emitted
                // bytecode reflects its structure (not a runtime re-parse).
                let sub_expr = parse_template_expr(expr_str, t.pos.clone())?;
                compile_expr(&sub_expr, chunk, resolutions)?;
                chunk.write_op(Opcode::ToString, t.pos.clone());
                if segments_emitted > 0 {
                    chunk.write_op(Opcode::Concat, t.pos.clone());
                }
                segments_emitted += 1;
            }
            i = end + 1;
            continue;
        }
        // Collect a run of literal chars up to the next `${`.
        let start = i;
        while i < bytes.len() && !(i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{')
        {
            i += 1;
        }
        let text = crate::evaluator::string_lit::unescape_string(&inner[start..i]);
        let idx = chunk.add_constant(str_obj(text));
        emit_const(chunk, idx, t.pos.clone());
        if segments_emitted > 0 {
            chunk.write_op(Opcode::Concat, t.pos.clone());
        }
        segments_emitted += 1;
    }
    // Empty template → empty string.
    if segments_emitted == 0 {
        let idx = chunk.add_constant(str_obj(""));
        emit_const(chunk, idx, t.pos.clone());
    }
    Ok(())
}

/// Re-parse a template `${...}` sub-expression string into an AST Expr, so the
/// compiler can emit bytecode for it (rather than deferring to a runtime
/// re-parse). Mirrors the tree-walker's `eval_template_expression` parse step.
fn parse_template_expr(src: &str, pos: crate::ast::Position) -> Result<Expr, Object> {
    let wrap = format!("let __gts_tpl = {};", src);
    let lex = Lexer::new(&wrap);
    let mut parser = Parser::new(lex, pos.file.as_ref());
    let prog = parser.parse_program();
    if !parser.errors().is_empty() || !prog.errors.is_empty() {
        return Err(unsupported(pos, "template expression parse error"));
    }
    // Extract the initializer expression from `let __gts_tpl = <expr>;`.
    for stmt in &prog.body {
        if let Stmt::Let(l) = stmt {
            if let Some(v) = &l.value {
                return Ok(v.clone());
            }
        }
    }
    Err(unsupported(pos, "template expression parse error"))
}

/// Find the matching `}` for a `${...}` template expression, accounting for
/// nested braces and quoted strings. Mirrors the tree-walker's helper.
fn find_template_expr_end(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut quote: u8 = 0;
    let mut escape = false;
    let mut i = start;
    while i < bytes.len() {
        let ch = bytes[i];
        if quote != 0 {
            if escape {
                escape = false;
            } else if ch == b'\\' {
                escape = true;
            } else if ch == quote {
                quote = 0;
            }
            i += 1;
            continue;
        }
        match ch {
            b'"' | b'\'' => quote = ch,
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Patch a jump placeholder to an explicit target offset (not necessarily
/// "here").
fn patch_jump_to(chunk: &mut Chunk, operand_ip: u32, target: u32) {
    let ip = operand_ip as usize;
    let bytes = target.to_be_bytes();
    chunk.code[ip] = bytes[0];
    chunk.code[ip + 1] = bytes[1];
    chunk.code[ip + 2] = bytes[2];
    chunk.code[ip + 3] = bytes[3];
}

/// Compile a function body into a sub-Chunk, register a `FunctionProto` on
/// the *parent* chunk's proto table, and return the proto index.
///
/// The body is compiled with its own statement stream and a trailing RETURN
/// (returning the last statement's value, or Undefined). Parameters are bound
/// by the interpreter at call time into the call environment. Stage 4.2 also
/// attaches the lexical upvalue descriptors; 4.4 will lower matching reads and
/// writes from dynamic names to slot/upvalue opcodes.
pub(crate) fn compile_method_proto(
    name: &str,
    params: Vec<crate::ast::Param>,
    body: crate::ast::BlockStmt,
    is_async: bool,
    return_t: Option<crate::ast::TypeAnnotation>,
    pos: crate::ast::Position,
    resolutions: &ResolutionMap,
) -> Result<Rc<FunctionProto>, Object> {
    let mut sub = Chunk::new();
    let mut loops: Vec<LoopFrame> = Vec::new();
    let n = body.statements.len();
    for (i, stmt) in body.statements.iter().enumerate() {
        compile_stmt(stmt, &mut sub, &mut loops, i + 1 == n, resolutions)?;
    }
    // If the body didn't end in an explicit RETURN, emit one so the call
    // always returns (the last value, or Undefined).
    if !matches_last_opcode(&sub, Opcode::Return) {
        sub.write_op(Opcode::Return, pos.clone());
    }
    let upvalue_desc = resolutions
        .function(name, &pos)
        .map(|resolution| resolution.upvalues.clone())
        .unwrap_or_default();
    let proto = FunctionProto::with_upvalues(
        name,
        params,
        body,
        is_async,
        false,
        return_t,
        pos,
        upvalue_desc,
    );
    *proto.chunk.borrow_mut() = Some(Rc::new(sub));
    Ok(proto)
}

#[allow(clippy::too_many_arguments)]
fn compile_function_proto(
    name: &str,
    params: Vec<crate::ast::Param>,
    body: crate::ast::BlockStmt,
    is_async: bool,
    lexical_this: bool,
    return_t: Option<crate::ast::TypeAnnotation>,
    pos: crate::ast::Position,
    parent: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<u16, Object> {
    let proto = if lexical_this {
        compile_lexical_function_proto(name, params, body, is_async, return_t, pos, resolutions)?
    } else {
        compile_method_proto(name, params, body, is_async, return_t, pos, resolutions)?
    };
    let idx = parent.protos.len() as u16;
    parent.protos.push(proto);
    Ok(idx)
}

fn compile_lexical_function_proto(
    name: &str,
    params: Vec<crate::ast::Param>,
    body: crate::ast::BlockStmt,
    is_async: bool,
    return_t: Option<crate::ast::TypeAnnotation>,
    pos: crate::ast::Position,
    resolutions: &ResolutionMap,
) -> Result<Rc<FunctionProto>, Object> {
    let proto = compile_method_proto(name, params, body, is_async, return_t, pos, resolutions)?;
    let rebuilt = FunctionProto::with_upvalues(
        proto.name.clone(),
        proto.params.clone(),
        (*proto.body).clone(),
        proto.is_async,
        true,
        proto.return_t.clone(),
        proto.pos.clone(),
        proto.upvalue_desc.clone(),
    );
    *rebuilt.chunk.borrow_mut() = proto.chunk.borrow().clone();
    Ok(rebuilt)
}

/// True if the last instruction in the chunk is `op`.
fn matches_last_opcode(chunk: &Chunk, op: Opcode) -> bool {
    // Walk backwards skipping operand bytes is hard; instead scan forward with
    // known operand widths. For stage 3 the only opcodes with operands in a
    // function body are Const/LoadName/StoreName/AssignName/Call/Closure (u16)
    // and Jump/JumpIfFalse/JumpIfTrue/Loop (u32). Simpler: track the opcode
    // positions by scanning.
    let mut ip = 0;
    let mut last_op = None;
    while ip < chunk.code.len() {
        let b = chunk.code[ip];
        last_op = Opcode::from_byte(b);
        ip += 1;
        // skip operands based on the opcode
        if let Some(o) = last_op {
            ip += operand_width(o) as usize;
        }
    }
    last_op == Some(op)
}

/// Byte width of the operand for an opcode (0 if none).
fn operand_width(op: Opcode) -> u8 {
    match op {
        Opcode::Const
        | Opcode::LoadName
        | Opcode::StoreName
        | Opcode::AssignName
        | Opcode::LoadGlobal
        | Opcode::StoreGlobal
        | Opcode::GetProperty
        | Opcode::SetProperty
        | Opcode::DefineMethod
        | Opcode::NewClass
        | Opcode::SuperMethod
        | Opcode::NewArray
        | Opcode::New
        | Opcode::Call
        | Opcode::Closure
        | Opcode::ImportModule
        | Opcode::ExportName => 2,
        Opcode::StoreTypedName => 4,
        Opcode::Jump | Opcode::JumpIfFalse | Opcode::JumpIfTrue | Opcode::Loop => 4,
        Opcode::LoadLocal | Opcode::StoreLocal | Opcode::LoadUpvalue | Opcode::StoreUpvalue => 1,
        _ => 0,
    }
}
///
/// Stage 1 keeps all variables in the (root) environment's name table, so a
/// declaration evaluates its initializer (if any) and emits a STORE_NAME.
/// `const` is recorded so a later assignment raises the matching TypeError;
/// the const-ness is tracked by the environment binding, not the opcode.
fn compile_decl(
    name: &str,
    value: Option<&Expr>,
    type_anno: Option<&crate::ast::TypeAnnotation>,
    is_const: bool,
    pos: crate::ast::Position,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    if let Some(v) = value {
        compile_expr(v, chunk, resolutions)?;
    } else {
        // Declaration without initializer → undefined.
        let idx = chunk.add_constant(Object::Undefined);
        emit_const(chunk, idx, pos.clone());
    }
    let name_idx = chunk.add_constant(str_obj(name));
    // Encode const-ness in the high bit of the name index operand so the
    // interpreter knows which binding flavor to create. (Name pools stay
    // small; a u16 with a flag bit is plenty.)
    let operand = if is_const {
        name_idx | 0x8000
    } else {
        name_idx
    };
    if let Some(type_anno) = type_anno {
        let type_idx = chunk.types.len() as u16;
        chunk.types.push(type_anno.clone());
        chunk.write_op(Opcode::StoreTypedName, pos.clone());
        chunk.write_u16(operand, pos.clone());
        chunk.write_u16(type_idx, pos);
    } else {
        chunk.write_op(Opcode::StoreName, pos.clone());
        chunk.write_u16(operand, pos);
    }
    Ok(())
}

/// Compile a destructuring declaration (B3.2): evaluate the source once, then
/// bind each element via Dup-source + GetIndex/GetProperty (+ default) + Store.
/// `is_const` flags const-ness in each StoreName operand.
///
/// Array `[a, b = d]`: for each index i,
///   DUP ; CONST i ; GET_INDEX ; [<default>] ; STORE_NAME a   ; (src kept)
/// then POP the source at the end.
/// Object `{x, y: z = d}`: same with GET_PROPERTY.
///
/// Rest (`...rest`) is not supported (no slice opcode) and returns a compile
/// error here; the tree-walker mirrors this to keep parity.
fn compile_destructure(
    binding: &crate::ast::BindingPattern,
    value: Option<&Expr>,
    pos: crate::ast::Position,
    is_const: bool,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    // Evaluate the source once; it stays on the stack across all bindings.
    if let Some(v) = value {
        compile_expr(v, chunk, resolutions)?;
    } else {
        let idx = chunk.add_constant(Object::Undefined);
        emit_const(chunk, idx, pos.clone());
    }

    let store = |name: &str, chunk: &mut Chunk, pos: &crate::ast::Position| {
        let name_idx = chunk.add_constant(str_obj(name.to_string()));
        let operand = if is_const {
            name_idx | 0x8000
        } else {
            name_idx
        };
        chunk.write_op(Opcode::StoreName, pos.clone());
        chunk.write_u16(operand, pos.clone());
    };

    match binding {
        crate::ast::BindingPattern::Array(elems) => {
            for (i, elem) in elems.iter().enumerate() {
                if elem.is_rest {
                    // `...rest`: collect elements [i..] into a new array.
                    // DUP the source so ArraySliceFrom consumes the copy while
                    // the original survives for the final Pop below.
                    chunk.write_op(Opcode::Dup, pos.clone()); // [src, src]
                    let start_const = chunk.add_constant(crate::object::num_obj(i as f64));
                    chunk.write_op(Opcode::Const, pos.clone());
                    chunk.write_u16(start_const, pos.clone()); // [src, src, start]
                    chunk.write_op(Opcode::ArraySliceFrom, pos.clone()); // [src, tail]
                    store(&elem.name, chunk, &pos); // pops tail → [src]
                    break;
                }
                if elem.name.is_empty() {
                    continue; // hole: still consumes a source index position
                }
                // DUP source ; CONST i ; GET_INDEX → value on top, source below.
                // (StoreName POPS the value, so source survives untouched.)
                chunk.write_op(Opcode::Dup, pos.clone());
                let idx_const = chunk.add_constant(crate::object::num_obj(i as f64));
                chunk.write_op(Opcode::Const, pos.clone());
                chunk.write_u16(idx_const, pos.clone());
                chunk.write_op(Opcode::GetIndex, pos.clone()); // [src, val]
                if let Some(def) = &elem.default {
                    emit_undefined_replace(def, chunk, resolutions, &pos)?;
                }
                store(&elem.name, chunk, &pos); // pops val → [src]
            }
        }
        crate::ast::BindingPattern::Object(elems) => {
            for elem in elems {
                chunk.write_op(Opcode::Dup, pos.clone()); // [src, src]
                let key_idx = chunk.add_constant(str_obj(elem.key.clone()));
                chunk.write_op(Opcode::GetProperty, pos.clone());
                chunk.write_u16(key_idx, pos.clone()); // [src, val]
                if let Some(def) = &elem.default {
                    emit_undefined_replace(def, chunk, resolutions, &pos)?;
                }
                store(&elem.target, chunk, &pos); // pops val → [src]
            }
        }
    }
    // Drop the original source.
    chunk.write_op(Opcode::Pop, pos);
    Ok(())
}

/// If the value on top of the stack is `undefined`, replace it with the
/// compiled default expression; otherwise keep it (0, "", null are KEPT —
/// only `undefined` triggers the default, matching JS/GoScript semantics).
///
/// stack: [val] → [val-or-default]
///   DUP ; CONST undefined ; STRICT_EQ ; JUMP_IF_FALSE keep ; POP ; <default> ; keep:
fn emit_undefined_replace(
    default: &Expr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    pos: &crate::ast::Position,
) -> Result<(), Object> {
    chunk.write_op(Opcode::Dup, pos.clone()); // [val, val]
    let und = chunk.add_constant(Object::Undefined);
    chunk.write_op(Opcode::Const, pos.clone());
    chunk.write_u16(und, pos.clone()); // [val, val, undefined]
    chunk.write_op(Opcode::Eq, pos.clone()); // [val, bool]
                                             // bool is true ⇒ val === undefined ⇒ replace. Jump past replace when false.
    let keep_ip = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, pos.clone());
    chunk.write_op(Opcode::Pop, pos.clone()); // drop the undefined val
    compile_expr(default, chunk, resolutions)?; // push default
    patch_jump_here(chunk, keep_ip);
    Ok(())
}

fn compile_expr(expr: &Expr, chunk: &mut Chunk, resolutions: &ResolutionMap) -> Result<(), Object> {
    match expr {
        // —— identifier read ——
        Expr::Ident(i) => {
            let name_idx = chunk.add_constant(str_obj(i.name.clone()));
            chunk.write_op(Opcode::LoadName, i.pos.clone());
            chunk.write_u16(name_idx, i.pos.clone());
            Ok(())
        }
        // —— assignment `name = expr` (and compound `+=` etc.) ——
        Expr::Assign(a) => compile_assign(a, chunk, resolutions),

        // —— literals ——
        Expr::Number(n) => emit_value_constant(chunk, num_obj(n.value), n.pos.clone()),
        Expr::Bool(b) => emit_value_constant(chunk, bool_obj(b.value), b.pos.clone()),
        Expr::Null(n) => emit_value_constant(chunk, Object::Null, n.pos.clone()),
        Expr::Undefined(u) => emit_value_constant(chunk, Object::Undefined, u.pos.clone()),
        Expr::String(s) => {
            // String literals are pure (escape processing only, no env), so
            // evaluate them at compile time and intern the result.
            let value = eval_string_lit(s);
            if value.is_runtime_error() {
                return Err(value);
            }
            let idx = chunk.add_constant(value);
            emit_const(chunk, idx, s.pos.clone());
            Ok(())
        }
        Expr::Regexp(r) => {
            // Regexp literals compile to a RegexpData value (pure).
            let value = eval_regexp_lit(r);
            if value.is_runtime_error() {
                return Err(value);
            }
            let idx = chunk.add_constant(value);
            emit_const(chunk, idx, r.pos.clone());
            Ok(())
        }
        Expr::Template(t) => {
            // Templates with `${...}` interpolation are lowered to a series of
            // string concatenations: each literal text segment is a CONST
            // string, each `${expr}` segment is the expression followed by
            // TO_STRING. All parts are joined with `+` (string concat).
            if !t.literal.contains("${") {
                // Static template (no interpolation): reduce at compile time.
                let value = crate::evaluator::string_lit::eval_template_static(t);
                let idx = chunk.add_constant(value);
                emit_const(chunk, idx, t.pos.clone());
                return Ok(());
            }
            compile_template_interpolated(t, chunk, resolutions)
        }
        Expr::Match(m) => compile_match(m, chunk, resolutions),
        // Dynamic `import(specifier)` → a Promise resolving to the module's
        // namespace object. The specifier must be a compile-time string.
        Expr::DynamicImport(d) => {
            let source = match &d.source {
                Expr::String(s) => crate::evaluator::eval_core::strip_quotes(&s.literal),
                Expr::Template(t) => crate::evaluator::eval_core::strip_quotes(&t.literal),
                _ => {
                    return Err(unsupported(
                        d.pos.clone(),
                        "dynamic import() requires a string specifier",
                    ));
                }
            };
            let source_idx = chunk.add_constant(str_obj(source));
            chunk.write_op(Opcode::ImportModule, d.pos.clone());
            chunk.write_u16(source_idx, d.pos.clone());
            chunk.write_op(Opcode::WrapResolvedPromise, d.pos.clone());
            Ok(())
        }
        Expr::Await(a) => {
            compile_expr(&a.value, chunk, resolutions)?;
            chunk.write_op(Opcode::Await, a.pos.clone());
            Ok(())
        }
        Expr::Array(a) => {
            if a.elements
                .iter()
                .any(|element| matches!(element, Expr::Spread(_)))
            {
                chunk.write_op(Opcode::NewArray, a.pos.clone());
                chunk.write_u16(0, a.pos.clone());
                for element in &a.elements {
                    match element {
                        Expr::Spread(sp) => {
                            compile_expr(&sp.value, chunk, resolutions)?;
                            chunk.write_op(Opcode::Spread, sp.pos.clone());
                        }
                        _ => {
                            compile_expr(element, chunk, resolutions)?;
                            chunk.write_op(Opcode::PushArg, element.pos());
                        }
                    }
                }
                return Ok(());
            }
            for element in &a.elements {
                compile_expr(element, chunk, resolutions)?;
            }
            chunk.write_op(Opcode::NewArray, a.pos.clone());
            chunk.write_u16(a.elements.len() as u16, a.pos.clone());
            Ok(())
        }
        Expr::Object(o) => {
            chunk.write_op(Opcode::NewObject, o.pos.clone());
            for prop in &o.properties {
                if prop.is_accessor {
                    return Err(unsupported(prop.pos.clone(), "object accessor property"));
                }
                if prop.spread {
                    compile_expr(&prop.value, chunk, resolutions)?;
                    chunk.write_op(Opcode::Spread, prop.pos.clone());
                    continue;
                }
                chunk.write_op(Opcode::Dup, prop.pos.clone());
                if prop.computed {
                    compile_expr(&prop.key, chunk, resolutions)?;
                    compile_expr(&prop.value, chunk, resolutions)?;
                    chunk.write_op(Opcode::SetIndex, prop.pos.clone());
                } else {
                    compile_expr(&prop.value, chunk, resolutions)?;
                    let key = object_property_key(prop)?;
                    let key_idx = chunk.add_constant(str_obj(key));
                    chunk.write_op(Opcode::SetProperty, prop.pos.clone());
                    chunk.write_u16(key_idx, prop.pos.clone());
                }
                chunk.write_op(Opcode::Pop, prop.pos.clone());
            }
            Ok(())
        }

        // —— prefix ——
        Expr::Prefix(p) => {
            // `++x` / `--x` (update operator, B3.1) — result is the new value.
            match p.op.as_str() {
                "++" => {
                    return compile_update_operator(
                        &p.right,
                        true,
                        Opcode::Add,
                        p.pos.clone(),
                        chunk,
                        resolutions,
                    );
                }
                "--" => {
                    return compile_update_operator(
                        &p.right,
                        true,
                        Opcode::Sub,
                        p.pos.clone(),
                        chunk,
                        resolutions,
                    );
                }
                "delete" => {
                    // `delete x` evaluates its operand (for side effects) and
                    // returns `true` (tree-walker parity: it does NOT actually
                    // remove properties). Lower: <operand> ; POP ; CONST true.
                    compile_expr(&p.right, chunk, resolutions)?;
                    chunk.write_op(Opcode::Pop, p.pos.clone());
                    let true_idx = chunk.add_constant(Object::Boolean(true));
                    chunk.write_op(Opcode::Const, p.pos.clone());
                    chunk.write_u16(true_idx, p.pos.clone());
                    return Ok(());
                }
                _ => {}
            }
            compile_expr(&p.right, chunk, resolutions)?;
            let op = match p.op.as_str() {
                "!" => Opcode::Not,
                "-" => Opcode::Neg,
                "~" => Opcode::BitNot,
                "typeof" => Opcode::TypeOf,
                "+" => Opcode::Identity,
                "void" => {
                    // `void x` evaluates its operand (for side effects) and
                    // returns undefined. Lower: <operand> ; POP ; CONST undefined.
                    chunk.write_op(Opcode::Pop, p.pos.clone());
                    let und_idx = chunk.add_constant(Object::Undefined);
                    chunk.write_op(Opcode::Const, p.pos.clone());
                    chunk.write_u16(und_idx, p.pos.clone());
                    return Ok(());
                }
                _ => {
                    return Err(unsupported(
                        p.pos.clone(),
                        &format!("prefix operator `{}`", p.op),
                    ));
                }
            };
            chunk.write_op(op, p.pos.clone());
            Ok(())
        }

        // —— infix ——
        Expr::Infix(i) => {
            // `x++` / `x--` (postfix update, B3.1) — result is the old value.
            if i.right.is_none() && (i.op == "++" || i.op == "--") {
                let delta_op = if i.op == "++" {
                    Opcode::Add
                } else {
                    Opcode::Sub
                };
                return compile_update_operator(
                    &i.left,
                    false,
                    delta_op,
                    i.pos.clone(),
                    chunk,
                    resolutions,
                );
            }
            if i.right.is_none() {
                return Err(unsupported(
                    i.pos.clone(),
                    "postfix update operator (++/--)",
                ));
            }
            match i.op.as_str() {
                "&&" => {
                    compile_expr(&i.left, chunk, resolutions)?;
                    compile_and(i, chunk, resolutions)
                }
                "||" => {
                    compile_expr(&i.left, chunk, resolutions)?;
                    compile_or(i, chunk, resolutions)
                }
                "??" => {
                    compile_expr(&i.left, chunk, resolutions)?;
                    compile_nullish_coalescing(i, chunk, resolutions)
                }
                _ => {
                    compile_expr(&i.left, chunk, resolutions)?;
                    compile_expr(i.right.as_ref().unwrap(), chunk, resolutions)?;
                    let op = binary_opcode(&i.op).ok_or_else(|| {
                        unsupported(i.pos.clone(), &format!("infix operator `{}`", i.op))
                    })?;
                    chunk.write_op(op, i.pos.clone());
                    Ok(())
                }
            }
        }
        Expr::Ternary(t) => compile_ternary(t, chunk, resolutions),

        // —— function call (callee + args, then CALL) ——
        Expr::Call(c) => {
            if let Expr::Super(_) = &c.callee {
                chunk.write_op(Opcode::LoadThis, c.pos.clone());
                let name_idx = chunk.add_constant(str_obj("constructor"));
                chunk.write_op(Opcode::SuperMethod, c.pos.clone());
                chunk.write_u16(name_idx, c.pos.clone());
                for arg in &c.args {
                    compile_expr(arg, chunk, resolutions)?;
                }
                let arg_count = c.args.len() as u16;
                chunk.write_op(Opcode::Call, c.pos.clone());
                chunk.write_u16(
                    encode_call_arg_count(arg_count, true, c.pos.clone())?,
                    c.pos.clone(),
                );
                return Ok(());
            }
            let has_this_receiver = compile_call_callee(&c.callee, chunk, resolutions)?;
            compile_call_args(
                &c.args,
                has_this_receiver,
                c.pos.clone(),
                chunk,
                resolutions,
            )
        }
        Expr::Optional(o) => compile_optional(o, chunk, resolutions),
        Expr::Member(m) => {
            compile_expr(&m.object, chunk, resolutions)?;
            if m.computed {
                compile_expr(&m.property, chunk, resolutions)?;
                chunk.write_op(Opcode::GetIndex, m.pos.clone());
            } else {
                let name = object_property_key_expr(&m.property);
                if name.is_empty() {
                    return Err(unsupported(m.pos.clone(), "member property key"));
                }
                let name_idx = chunk.add_constant(str_obj(name));
                chunk.write_op(Opcode::GetProperty, m.pos.clone());
                chunk.write_u16(name_idx, m.pos.clone());
            }
            Ok(())
        }
        Expr::Index(i) => {
            compile_expr(&i.left, chunk, resolutions)?;
            compile_expr(&i.index, chunk, resolutions)?;
            chunk.write_op(Opcode::GetIndex, i.pos.clone());
            Ok(())
        }
        Expr::New(n) => {
            compile_expr(&n.callee, chunk, resolutions)?;
            for arg in &n.args {
                compile_expr(arg, chunk, resolutions)?;
            }
            chunk.write_op(Opcode::New, n.pos.clone());
            chunk.write_u16(n.args.len() as u16, n.pos.clone());
            Ok(())
        }
        Expr::This(t) => {
            chunk.write_op(Opcode::LoadThis, t.pos.clone());
            Ok(())
        }
        Expr::Super(s) => {
            if s.method.is_empty() {
                let idx = chunk.add_constant(Object::Undefined);
                emit_const(chunk, idx, s.pos.clone());
            } else {
                let name_idx = chunk.add_constant(str_obj(s.method.clone()));
                chunk.write_op(Opcode::SuperMethod, s.pos.clone());
                chunk.write_u16(name_idx, s.pos.clone());
            }
            Ok(())
        }
        Expr::Class(c) => {
            let class_idx = add_class_decl(chunk, (**c).clone());
            chunk.write_op(Opcode::NewClass, c.pos.clone());
            chunk.write_u16(class_idx, c.pos.clone());
            Ok(())
        }
        // —— function expression ——
        Expr::Func(f) => {
            let idx = compile_function_proto(
                &f.name,
                f.params.clone(),
                f.body.clone(),
                f.is_async,
                false,
                f.return_t.clone(),
                f.pos.clone(),
                chunk,
                resolutions,
            )?;
            chunk.write_op(Opcode::Closure, f.pos.clone());
            chunk.write_u16(idx, f.pos.clone());
            Ok(())
        }
        // —— arrow function ——
        Expr::Arrow(a) => {
            // Arrow body: either an expression (implicit return) or a block.
            let body = match &a.body {
                crate::ast::ArrowBody::Expr(e) => {
                    // Wrap the expression in a single return statement.
                    crate::ast::BlockStmt {
                        pos: a.pos.clone(),
                        statements: vec![Stmt::Return(crate::ast::ReturnStmt {
                            pos: a.pos.clone(),
                            value: Some(e.clone()),
                        })],
                    }
                }
                crate::ast::ArrowBody::Block(b) => b.clone(),
            };
            let idx = compile_function_proto(
                "",
                a.params.clone(),
                body,
                a.is_async,
                true, // arrow functions capture `this` lexically
                a.return_t.clone(),
                a.pos.clone(),
                chunk,
                resolutions,
            )?;
            chunk.write_op(Opcode::Closure, a.pos.clone());
            chunk.write_u16(idx, a.pos.clone());
            Ok(())
        }
        Expr::Spread(sp) => Err(unsupported(
            sp.pos.clone(),
            "bare spread expression outside array/object/call context",
        )),
    }
}

fn compile_ternary(
    t: &crate::ast::TernaryExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    compile_expr(&t.cond, chunk, resolutions)?;
    let to_alternate = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, t.pos.clone());
    compile_expr(&t.consequent, chunk, resolutions)?;
    let to_end = emit_jump_placeholder(chunk, Opcode::Jump, t.pos.clone());
    patch_jump_here(chunk, to_alternate);
    compile_expr(&t.alternate, chunk, resolutions)?;
    patch_jump_here(chunk, to_end);
    Ok(())
}

fn compile_optional(
    o: &crate::ast::OptionalExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    compile_expr(&o.object, chunk, resolutions)?;
    let nullish_jumps = emit_nullish_jump_checks(chunk, o.pos.clone());

    if o.is_call {
        compile_call_args(&o.args, false, o.pos.clone(), chunk, resolutions)?;
    } else if o.computed {
        compile_expr(&o.property, chunk, resolutions)?;
        chunk.write_op(Opcode::GetIndex, o.pos.clone());
    } else {
        let name = object_property_key_expr(&o.property);
        if name.is_empty() {
            return Err(unsupported(o.pos.clone(), "optional property key"));
        }
        let name_idx = chunk.add_constant(str_obj(name));
        chunk.write_op(Opcode::GetProperty, o.pos.clone());
        chunk.write_u16(name_idx, o.pos.clone());
    }

    let to_end = emit_jump_placeholder(chunk, Opcode::Jump, o.pos.clone());
    let nullish_ip = chunk.code.len() as u32;
    for jump in nullish_jumps {
        patch_jump_to(chunk, jump, nullish_ip);
    }
    chunk.write_op(Opcode::Pop, o.pos.clone());
    let undefined_idx = chunk.add_constant(Object::Undefined);
    emit_const(chunk, undefined_idx, o.pos.clone());
    patch_jump_here(chunk, to_end);
    Ok(())
}

fn emit_nullish_jump_checks(chunk: &mut Chunk, pos: crate::ast::Position) -> Vec<u32> {
    chunk.write_op(Opcode::Dup, pos.clone());
    let null_idx = chunk.add_constant(Object::Null);
    emit_const(chunk, null_idx, pos.clone());
    chunk.write_op(Opcode::Eq, pos.clone());
    let null_jump = emit_jump_placeholder(chunk, Opcode::JumpIfTrue, pos.clone());

    chunk.write_op(Opcode::Dup, pos.clone());
    let undefined_idx = chunk.add_constant(Object::Undefined);
    emit_const(chunk, undefined_idx, pos.clone());
    chunk.write_op(Opcode::Eq, pos.clone());
    let undefined_jump = emit_jump_placeholder(chunk, Opcode::JumpIfTrue, pos);

    vec![null_jump, undefined_jump]
}

fn compile_call_args(
    args: &[Expr],
    has_this_receiver: bool,
    pos: crate::ast::Position,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    if args.iter().any(|arg| matches!(arg, Expr::Spread(_))) {
        chunk.write_op(Opcode::NewArray, pos.clone());
        chunk.write_u16(0, pos.clone());
        for arg in args {
            match arg {
                Expr::Spread(sp) => {
                    compile_expr(&sp.value, chunk, resolutions)?;
                    chunk.write_op(Opcode::Spread, sp.pos.clone());
                }
                _ => {
                    compile_expr(arg, chunk, resolutions)?;
                    chunk.write_op(Opcode::PushArg, arg.pos());
                }
            }
        }
        chunk.write_op(Opcode::CallSpread, pos);
        return Ok(());
    }

    for arg in args {
        compile_expr(arg, chunk, resolutions)?;
    }
    let arg_count = args.len() as u16;
    chunk.write_op(Opcode::Call, pos.clone());
    chunk.write_u16(
        encode_call_arg_count(arg_count, has_this_receiver, pos.clone())?,
        pos,
    );
    Ok(())
}

fn compile_match(
    m: &crate::ast::MatchExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    let subject_name = format!("__gts_bc_match_subject_{}_{}", m.pos.line, m.pos.col);
    compile_expr(&m.expr, chunk, resolutions)?;
    let subject_idx = chunk.add_constant(str_obj(subject_name.clone()));
    chunk.write_op(Opcode::StoreName, m.pos.clone());
    chunk.write_u16(subject_idx, m.pos.clone());

    let mut to_end = Vec::new();
    for arm in &m.arms {
        emit_load_name(chunk, &subject_name, arm.pos.clone());
        compile_pattern_test(&arm.pattern, chunk, resolutions)?;
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
            compile_match_body(&arm.body, chunk, resolutions)?;
            to_end.push(emit_jump_placeholder(chunk, Opcode::Jump, arm.pos.clone()));
            patch_jump_here(chunk, guard_failed);
        } else {
            compile_match_body(&arm.body, chunk, resolutions)?;
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
                compile_stmt(stmt, chunk, &mut Vec::new(), i + 1 == n, resolutions)?;
            }
            Ok(())
        }
    }
}

fn compile_pattern_test(
    pattern: &Pattern,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
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
                compile_pattern_test(alt, chunk, resolutions)?;
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

fn emit_load_name(chunk: &mut Chunk, name: &str, pos: crate::ast::Position) {
    let idx = chunk.add_constant(str_obj(name.to_string()));
    chunk.write_op(Opcode::LoadName, pos.clone());
    chunk.write_u16(idx, pos);
}

fn add_class_decl(chunk: &mut Chunk, decl: crate::ast::ClassDecl) -> u16 {
    let idx = chunk.classes.len() as u16;
    chunk.classes.push(Rc::new(decl));
    idx
}

fn object_property_key(prop: &crate::ast::Property) -> Result<String, Object> {
    if prop.shorthand {
        if let Expr::Ident(i) = &prop.key {
            return Ok(i.name.clone());
        }
    }
    let key = object_property_key_expr(&prop.key);
    if key.is_empty() {
        Err(unsupported(prop.pos.clone(), "object property key"))
    } else {
        Ok(key)
    }
}

fn object_property_key_expr(expr: &Expr) -> String {
    match expr {
        Expr::Ident(i) => i.name.clone(),
        Expr::String(s) => crate::evaluator::eval_core::strip_quotes(&s.literal),
        Expr::Number(n) => crate::object::format_number(n.value),
        _ => String::new(),
    }
}

fn compile_call_callee(
    callee: &Expr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<bool, Object> {
    match callee {
        Expr::Member(m) if matches!(&m.object, Expr::Super(_)) => {
            chunk.write_op(Opcode::LoadThis, m.pos.clone());
            let name = object_property_key_expr(&m.property);
            if name.is_empty() {
                return Err(unsupported(m.pos.clone(), "super method key"));
            }
            let name_idx = chunk.add_constant(str_obj(name));
            chunk.write_op(Opcode::SuperMethod, m.pos.clone());
            chunk.write_u16(name_idx, m.pos.clone());
            Ok(true)
        }
        Expr::Member(m) if !m.computed => {
            compile_expr(&m.object, chunk, resolutions)?;
            chunk.write_op(Opcode::Dup, m.pos.clone());
            let name = object_property_key_expr(&m.property);
            if name.is_empty() {
                return Err(unsupported(m.pos.clone(), "member property key"));
            }
            let name_idx = chunk.add_constant(str_obj(name));
            chunk.write_op(Opcode::GetProperty, m.pos.clone());
            chunk.write_u16(name_idx, m.pos.clone());
            Ok(true)
        }
        Expr::Index(i) => {
            compile_expr(&i.left, chunk, resolutions)?;
            chunk.write_op(Opcode::Dup, i.pos.clone());
            compile_expr(&i.index, chunk, resolutions)?;
            chunk.write_op(Opcode::GetIndex, i.pos.clone());
            Ok(true)
        }
        _ => {
            compile_expr(callee, chunk, resolutions)?;
            Ok(false)
        }
    }
}

fn encode_call_arg_count(
    arg_count: u16,
    has_this_receiver: bool,
    pos: crate::ast::Position,
) -> Result<u16, Object> {
    if arg_count > 0x7fff {
        return Err(unsupported(pos, "call with more than 32767 arguments"));
    }
    Ok(arg_count | if has_this_receiver { 0x8000 } else { 0 })
}

/// Compile an assignment expression.
///
/// Stage 5 extends identifier assignment with member/index targets.
fn compile_assign(
    a: &crate::ast::AssignExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    match &a.left {
        Expr::Ident(i) => return compile_name_assign(a, &i.name, chunk, resolutions),
        Expr::Member(m) => return compile_member_assign(a, m, chunk, resolutions),
        Expr::Index(i) => return compile_index_assign(a, i, chunk, resolutions),
        _ => {}
    }
    Err(unsupported(a.pos.clone(), "assignment target"))
}

fn compile_name_assign(
    a: &crate::ast::AssignExpr,
    name: &str,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    if a.op == "=" {
        compile_expr(&a.right, chunk, resolutions)?;
        // DUP so the assigned value is both stored and left on the stack as
        // the expression result (assignment evaluates to the value).
        chunk.write_op(Opcode::Dup, a.pos.clone());
        let name_idx = chunk.add_constant(str_obj(name.to_string()));
        chunk.write_op(Opcode::AssignName, a.pos.clone());
        chunk.write_u16(name_idx, a.pos.clone());
        Ok(())
    } else {
        // Compound: read current, combine with right, store.
        // LOAD_NAME name ; <right> ; <op> ; DUP ; ASSIGN_NAME name
        let name_idx_load = chunk.add_constant(str_obj(name.to_string()));
        chunk.write_op(Opcode::LoadName, a.pos.clone());
        chunk.write_u16(name_idx_load, a.pos.clone());
        compile_expr(&a.right, chunk, resolutions)?;
        // Strip the `=` suffix to get the binary op (`+=` → `+`).
        let bin_op: String = a.op[..a.op.len() - 1].to_string();
        let op = binary_opcode(&bin_op).ok_or_else(|| {
            unsupported(a.pos.clone(), &format!("compound assignment `{}`", a.op))
        })?;
        chunk.write_op(op, a.pos.clone());
        chunk.write_op(Opcode::Dup, a.pos.clone());
        let name_idx_store = chunk.add_constant(str_obj(name.to_string()));
        chunk.write_op(Opcode::AssignName, a.pos.clone());
        chunk.write_u16(name_idx_store, a.pos.clone());
        Ok(())
    }
}

fn compile_member_assign(
    a: &crate::ast::AssignExpr,
    m: &crate::ast::MemberExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    compile_expr(&m.object, chunk, resolutions)?;
    if m.computed {
        compile_expr(&m.property, chunk, resolutions)?;
        compile_assign_rhs(a, chunk, resolutions)?;
        chunk.write_op(Opcode::SetIndex, a.pos.clone());
    } else {
        compile_assign_rhs(a, chunk, resolutions)?;
        let name = object_property_key_expr(&m.property);
        if name.is_empty() {
            return Err(unsupported(m.pos.clone(), "member property key"));
        }
        let name_idx = chunk.add_constant(str_obj(name));
        chunk.write_op(Opcode::SetProperty, a.pos.clone());
        chunk.write_u16(name_idx, a.pos.clone());
    }
    Ok(())
}

fn compile_index_assign(
    a: &crate::ast::AssignExpr,
    i: &crate::ast::IndexExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    compile_expr(&i.left, chunk, resolutions)?;
    compile_expr(&i.index, chunk, resolutions)?;
    compile_assign_rhs(a, chunk, resolutions)?;
    chunk.write_op(Opcode::SetIndex, a.pos.clone());
    Ok(())
}

fn compile_assign_rhs(
    a: &crate::ast::AssignExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    if a.op != "=" {
        return Err(unsupported(
            a.pos.clone(),
            &format!("compound assignment `{}` to member/index target", a.op),
        ));
    }
    compile_expr(&a.right, chunk, resolutions)
}

/// Compile an update operator `++`/`--` (B3.1).
///
/// `is_prefix`: `++x` (true, result is the NEW value) vs `x++` (false, result
/// is the OLD value). The delta opcode is `Opcode::Add` for `++`, `Sub` for `--`.
///
/// Lowering (AssignName/SetProperty peek-and-keep the value, so we manage the
/// stack explicitly):
///
/// Ident, prefix `++x` (result = new):
///   LOAD_NAME x ; CONST 1 ; ADD ; ASSIGN_NAME x     → stack: [new]
///   (AssignName stores `new` and leaves it as the result.)
///
/// Ident, postfix `x++` (result = old):
///   LOAD_NAME x ; DUP ; CONST 1 ; ADD ; ASSIGN_NAME x ; POP → stack: [old]
///   (DUP preserves old below; modify→store leaves [old, new]; POP drops new.)
///
/// Member/Index targets support prefix only (postfix on member/index is rare
/// and would need an extra temp); they error for postfix.
fn compile_update_operator(
    target: &Expr,
    is_prefix: bool,
    delta_op: Opcode,
    pos: crate::ast::Position,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    let one_idx = chunk.add_constant(crate::object::num_obj(1.0));
    match target {
        Expr::Ident(i) => {
            let name_idx = chunk.add_constant(str_obj(i.name.to_string()));
            // Load current value.
            chunk.write_op(Opcode::LoadName, pos.clone());
            chunk.write_u16(name_idx, pos.clone());
            if !is_prefix {
                // Postfix: preserve the OLD value below the upcoming new value.
                chunk.write_op(Opcode::Dup, pos.clone());
            }
            // Modify by ±1 → new value on top.
            chunk.write_op(Opcode::Const, pos.clone());
            chunk.write_u16(one_idx, pos.clone());
            chunk.write_op(delta_op, pos.clone());
            // Store the new value back (AssignName peeks, leaves it on stack).
            chunk.write_op(Opcode::AssignName, pos.clone());
            chunk.write_u16(name_idx, pos.clone());
            if !is_prefix {
                // Postfix: drop the new value, leaving the preserved old value.
                chunk.write_op(Opcode::Pop, pos.clone());
            }
            Ok(())
        }
        Expr::Member(m) => {
            if !is_prefix {
                return Err(unsupported(
                    pos.clone(),
                    "postfix ++/-- on member/index (use prefix or an ident)",
                ));
            }
            if m.computed {
                return Err(unsupported(
                    pos.clone(),
                    "++/-- on computed member (assign to a temp first)",
                ));
            }
            compile_expr(&m.object, chunk, resolutions)?; // [obj]
                                                          // Duplicate the receiver: GetProperty consumes one obj, the other
                                                          // survives for SetProperty.
            chunk.write_op(Opcode::Dup, pos.clone()); // [obj, obj]
            let name = object_property_key_expr(&m.property);
            if name.is_empty() {
                return Err(unsupported(m.pos.clone(), "member property key"));
            }
            let name_idx = chunk.add_constant(str_obj(name));
            chunk.write_op(Opcode::GetProperty, pos.clone());
            chunk.write_u16(name_idx, pos.clone()); // [obj(saved), val]
            chunk.write_op(Opcode::Const, pos.clone());
            chunk.write_u16(one_idx, pos.clone());
            chunk.write_op(delta_op, pos.clone()); // [obj(saved), new]
            chunk.write_op(Opcode::SetProperty, pos.clone());
            chunk.write_u16(name_idx, pos.clone()); // [new]
            Ok(())
        }
        Expr::Index(_) => Err(unsupported(
            pos.clone(),
            "++/-- on index target (assign to a temp first)",
        )),
        _ => Err(unsupported(pos.clone(), "update operator target")),
    }
}

/// Map a GTS infix operator string to its VM opcode. Returns `None` for
/// operators not yet supported (bitwise, etc.) so the caller emits a clean
/// compile error instead of broken bytecode.
fn binary_opcode(op: &str) -> Option<Opcode> {
    Some(match op {
        "+" => Opcode::Add,
        "-" => Opcode::Sub,
        "*" => Opcode::Mul,
        "/" => Opcode::Div,
        "%" => Opcode::Mod,
        "**" => Opcode::Pow,
        "&" => Opcode::BitAnd,
        "|" => Opcode::BitOr,
        "^" => Opcode::BitXor,
        "<<" => Opcode::Shl,
        ">>" => Opcode::Shr,
        ">>>" => Opcode::UShr,
        "===" => Opcode::Eq,
        "!==" => Opcode::Neq,
        "<" => Opcode::Lt,
        "<=" => Opcode::Le,
        ">" => Opcode::Gt,
        ">=" => Opcode::Ge,
        "instanceof" => Opcode::InstanceOf,
        "in" => Opcode::In,
        _ => return None,
    })
}

/// Lower `left && right`: keep left if falsy, else replace with right.
/// Pre: left is already on the stack.
fn compile_and(
    i: &crate::ast::InfixExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    let pos = i.pos.clone();
    //   <left>            ; stack: [L]
    //   DUP               ; stack: [L, L]
    //   JUMP_IF_FALSE end ; pops test, stack: [L]
    //   POP               ; stack: []
    //   <right>           ; stack: [R]
    //   end:
    chunk.write_op(Opcode::Dup, pos.clone());
    let patch_ip = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, pos.clone());
    chunk.write_op(Opcode::Pop, pos.clone());
    compile_expr(i.right.as_ref().unwrap(), chunk, resolutions)?;
    patch_jump_here(chunk, patch_ip);
    Ok(())
}

/// Lower `left || right`: keep left if truthy, else replace with right.
/// Pre: left is already on the stack.
fn compile_or(
    i: &crate::ast::InfixExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    let pos = i.pos.clone();
    //   <left>                  ; stack: [L]
    //   DUP                     ; stack: [L, L]
    //   JUMP_IF_FALSE eval_right; pops test, stack: [L]
    //   JUMP end                ; stack: [L] (truthy: keep)
    //   eval_right: POP         ; stack: []
    //   <right>                 ; stack: [R]
    //   end:
    chunk.write_op(Opcode::Dup, pos.clone());
    let to_right = emit_jump_placeholder(chunk, Opcode::JumpIfFalse, pos.clone());
    let to_end = emit_jump_placeholder(chunk, Opcode::Jump, pos.clone());
    patch_jump_here(chunk, to_right);
    chunk.write_op(Opcode::Pop, pos.clone());
    compile_expr(i.right.as_ref().unwrap(), chunk, resolutions)?;
    patch_jump_here(chunk, to_end);
    Ok(())
}

/// Lower `left ?? right`: keep left unless it is null or undefined.
/// Pre: left is already on the stack.
fn compile_nullish_coalescing(
    i: &crate::ast::InfixExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    let pos = i.pos.clone();
    //   <left>                       ; stack: [L]
    //   null/undefined checks        ; stack: [L]
    //   JUMP end                     ; stack: [L] (non-nullish: keep)
    //   nullish: POP                 ; stack: []
    //   <right>                      ; stack: [R]
    //   end:
    let nullish_jumps = emit_nullish_jump_checks(chunk, pos.clone());
    let to_end = emit_jump_placeholder(chunk, Opcode::Jump, pos.clone());
    let nullish_ip = chunk.code.len() as u32;
    for jump in nullish_jumps {
        patch_jump_to(chunk, jump, nullish_ip);
    }
    chunk.write_op(Opcode::Pop, pos.clone());
    compile_expr(i.right.as_ref().unwrap(), chunk, resolutions)?;
    patch_jump_here(chunk, to_end);
    Ok(())
}

/// Emit `<op> <placeholder u32>` and return the byte offset of the placeholder
/// (the opcode byte position), so the caller can patch it with `patch_jump_here`.
fn emit_jump_placeholder(chunk: &mut Chunk, op: Opcode, pos: crate::ast::Position) -> u32 {
    chunk.write_op(op, pos.clone());
    let patch = chunk.code.len() as u32;
    chunk.write_u32(0, pos);
    patch
}

/// Patch a jump placeholder (the u32 operand at byte offset `operand_ip`) to
/// point at the current code position.
fn patch_jump_here(chunk: &mut Chunk, operand_ip: u32) {
    let target = chunk.code.len() as u32;
    let ip = operand_ip as usize;
    let bytes = target.to_be_bytes();
    chunk.code[ip] = bytes[0];
    chunk.code[ip + 1] = bytes[1];
    chunk.code[ip + 2] = bytes[2];
    chunk.code[ip + 3] = bytes[3];
}

/// Emit a CONST opcode with its u16 operand.
fn emit_const(chunk: &mut Chunk, idx: u16, pos: crate::ast::Position) {
    chunk.write_op(Opcode::Const, pos.clone());
    chunk.write_u16(idx, pos);
}

/// Intern a compile-time-known value into the constant pool and emit a `Const`
/// instruction that loads it. Used by the scalar-literal arms of `compile_expr`
/// (Number/Bool/Null/Undefined), which all reduce to "constant + emit".
fn emit_value_constant(
    chunk: &mut Chunk,
    value: Object,
    pos: crate::ast::Position,
) -> Result<(), Object> {
    let idx = chunk.add_constant(value);
    emit_const(chunk, idx, pos);
    Ok(())
}

fn unsupported(pos: crate::ast::Position, what: &str) -> Object {
    new_error(
        pos,
        format!("CompileError: bytecode VM does not yet support {}", what),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn compile_src(src: &str) -> Chunk {
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer, "t.gs");
        let program = parser.parse_program();
        assert!(
            program.errors.is_empty(),
            "parse errors: {:?}",
            program.errors
        );
        compile(&program).expect("compile should succeed for stage-0 inputs")
    }

    #[test]
    fn compiles_literal_number() {
        let chunk = compile_src("42");
        assert_eq!(chunk.code[0], Opcode::Const as u8);
        assert!(matches!(chunk.constants[0], Object::Number(n) if n == 42.0));
        assert_eq!(*chunk.code.last().unwrap(), Opcode::Return as u8);
    }

    #[test]
    fn compiles_add_post_order() {
        // 1 + 2 + 3  ⇒  CONST 1, CONST 2, ADD, CONST 3, ADD, RETURN
        let chunk = compile_src("1 + 2 + 3");
        // Walk the instruction stream properly (don't flat-filter bytes: a
        // CONST operand byte could collide with an opcode value).
        let spine = decode_opcode_spine(&chunk);
        let expected = vec![
            Opcode::Const,
            Opcode::Const,
            Opcode::Add,
            Opcode::Const,
            Opcode::Add,
            Opcode::Return,
        ];
        assert_eq!(spine, expected);
    }

    /// Decode just the opcode bytes, skipping each instruction's operands.
    fn decode_opcode_spine(chunk: &Chunk) -> Vec<Opcode> {
        let mut out = Vec::new();
        let mut ip = 0;
        while ip < chunk.code.len() {
            let op = Opcode::from_byte(chunk.code[ip]).expect("valid opcode");
            out.push(op);
            ip += 1;
            ip += operand_width(op) as usize;
        }
        out
    }

    #[test]
    fn rejects_unsupported_node() {
        // Unsupported nodes must be refused rather than silently miscompiled.
        // Computed-member postfix update is not yet supported.
        let lexer = Lexer::new("let a = 1; a.b++");
        let mut parser = Parser::new(lexer, "t.gs");
        let program = parser.parse_program();
        let result = compile(&program);
        assert!(
            result.is_err(),
            "unsupported postfix update on member should not compile"
        );
    }

    #[test]
    fn compiles_throw_opcode() {
        let chunk = compile_src("throw \"boom\";");
        let spine = decode_opcode_spine(&chunk);
        assert!(spine.contains(&Opcode::Throw));
    }

    #[test]
    fn compiles_await_opcode() {
        let chunk = compile_src("await value;");
        let spine = decode_opcode_spine(&chunk);
        assert!(spine.contains(&Opcode::Await));
    }

    #[test]
    fn compiles_prefix_identity_opcode() {
        let chunk = compile_src("+42");
        let spine = decode_opcode_spine(&chunk);
        assert!(spine.contains(&Opcode::Identity));
    }

    #[test]
    fn compiles_ternary_branch_opcodes() {
        let chunk = compile_src("true ? 1 : 2");
        let spine = decode_opcode_spine(&chunk);
        assert!(spine.contains(&Opcode::JumpIfFalse));
        assert!(spine.contains(&Opcode::Jump));
    }

    #[test]
    fn compiles_optional_chain_nullish_checks() {
        let chunk = compile_src("let obj = null; obj?.name;");
        let spine = decode_opcode_spine(&chunk);
        assert!(spine.contains(&Opcode::Dup));
        assert!(spine.contains(&Opcode::JumpIfTrue));
        assert!(spine.contains(&Opcode::GetProperty));
    }

    #[test]
    fn compiles_nullish_coalescing_short_circuit() {
        let chunk = compile_src("null ?? 42");
        let spine = decode_opcode_spine(&chunk);
        assert!(spine.contains(&Opcode::Dup));
        assert!(spine.contains(&Opcode::JumpIfTrue));
        assert!(spine.contains(&Opcode::Jump));
    }

    #[test]
    fn compiles_prefix_increment_uses_load_assign() {
        // ++x → LOAD_NAME x ; CONST 1 ; ADD ; ASSIGN_NAME x
        let chunk = compile_src("let x = 1; ++x");
        let spine = decode_opcode_spine(&chunk);
        assert!(spine.contains(&Opcode::LoadName));
        assert!(spine.contains(&Opcode::Add));
        assert!(spine.contains(&Opcode::AssignName));
        // Prefix must NOT emit a Pop (the new value is the result as-is).
        let last_meaningful = spine
            .iter()
            .copied()
            .filter(|op| !matches!(op, Opcode::Const | Opcode::Return | Opcode::ReturnNull))
            .last();
        assert!(matches!(last_meaningful, Some(Opcode::AssignName)));
    }

    #[test]
    fn compiles_postfix_increment_preserves_old_value() {
        // x++ → LOAD_NAME x ; DUP ; CONST 1 ; ADD ; ASSIGN_NAME x ; POP
        // The trailing Pop drops the new value, leaving the old as the result.
        let chunk = compile_src("let x = 1; x++");
        let spine = decode_opcode_spine(&chunk);
        assert!(spine.contains(&Opcode::Dup));
        assert!(spine.contains(&Opcode::Add));
        assert!(spine.contains(&Opcode::Pop));
    }

    #[test]
    fn compiles_export_star_uses_import_export_all() {
        // `export * from "m"` → IMPORT_MODULE ; EXPORT_ALL
        let chunk = compile_src("export * from \"./m\"");
        let spine = decode_opcode_spine(&chunk);
        assert!(spine.contains(&Opcode::ImportModule));
        assert!(spine.contains(&Opcode::ExportAll));
    }

    #[test]
    fn compiles_dynamic_import_as_promise() {
        // `import("./m")` → IMPORT_MODULE ; WRAP_RESOLVED_PROMISE
        let chunk = compile_src("import(\"./m\")");
        let spine = decode_opcode_spine(&chunk);
        assert!(spine.contains(&Opcode::ImportModule));
        assert!(spine.contains(&Opcode::WrapResolvedPromise));
    }

    #[test]
    fn records_async_function_proto() {
        let chunk = compile_src("async function answer() { return 42; }");
        assert_eq!(chunk.protos.len(), 1);
        assert!(chunk.protos[0].is_async);
    }

    #[test]
    fn records_async_arrow_proto() {
        let chunk = compile_src("let answer = async (value) => value;");
        assert_eq!(chunk.protos.len(), 1);
        assert!(chunk.protos[0].is_async);
        assert!(chunk.protos[0].lexical_this);
    }

    #[test]
    fn compiles_typed_declaration_metadata() {
        let chunk = compile_src("let value: number = 1;");
        let spine = decode_opcode_spine(&chunk);
        assert!(spine.contains(&Opcode::StoreTypedName));
        assert_eq!(chunk.types.len(), 1);
        assert_eq!(chunk.types[0].to_string(), "number");
    }

    #[test]
    fn compiles_import_bindings_from_module_object() {
        let chunk = compile_src(r#"import def, { named, other as alias } from "mod";"#);
        let spine = decode_opcode_spine(&chunk);
        assert!(spine.contains(&Opcode::ImportModule));
        assert!(spine.contains(&Opcode::GetProperty));
        assert!(spine.contains(&Opcode::StoreName));
        assert!(chunk
            .constants
            .iter()
            .any(|value| matches!(value, Object::String(s) if s.as_ref() == "mod")));
    }

    #[test]
    fn compiles_export_declarations_to_export_name() {
        let chunk = compile_src("export const value = 42; export { value as answer };");
        let spine = decode_opcode_spine(&chunk);
        assert!(spine.contains(&Opcode::ExportName));
        assert!(chunk
            .constants
            .iter()
            .any(|value| matches!(value, Object::String(s) if s.as_ref() == "answer")));
    }

    #[test]
    fn records_try_protected_region() {
        let chunk = compile_src("try { 1; } catch (err) { 2; } finally { 3; }");
        assert_eq!(chunk.protected_regions.len(), 2);
        let region = &chunk.protected_regions[0];
        assert!(region.try_start < region.try_end);
        assert!(region.try_end < region.handler_ip);
        assert!(region.finally_ip.is_some());
        assert!(region.finally_ip.unwrap() > region.handler_ip);
        assert_eq!(region.catch_binding_slot, None);
        assert_eq!(
            Opcode::from_byte(chunk.code[region.handler_ip as usize]),
            Some(Opcode::StoreName)
        );
        let catch_region = &chunk.protected_regions[1];
        assert_eq!(catch_region.handler_ip, region.finally_ip.unwrap());
    }

    #[test]
    fn function_proto_records_resolved_upvalues() {
        let chunk = compile_src(
            "function outer() { let x = 1; function inner() { return x; } return inner; }",
        );
        let outer = chunk
            .protos
            .iter()
            .find(|proto| proto.name == "outer")
            .expect("outer proto");
        let outer_chunk = outer.chunk.borrow().clone().expect("outer chunk");
        let inner = outer_chunk
            .protos
            .iter()
            .find(|proto| proto.name == "inner")
            .expect("inner proto");

        assert_eq!(inner.upvalue_desc.len(), 1);
        assert_eq!(inner.upvalue_desc[0].name, "x");
        assert_eq!(
            inner.upvalue_desc[0].source,
            crate::bytecode::closure::UpvalueSource::LocalSlot(1)
        );
    }
}
