use crate::ast::{AssignExpr, Expr, IndexExpr, MemberExpr, Position};
use crate::object::{str_obj, Object};

use super::chunk::Chunk;
use super::compiler::unsupported;
use super::compiler_helpers::object_property_key_expr;
use super::compiler_operators::binary_opcode;
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

type CompileExprFn = fn(&Expr, &mut Chunk, &ResolutionMap) -> Result<(), Object>;

/// Compile an assignment expression.
///
/// Identifier assignment supports simple and compound forms. Member/index
/// targets support simple assignment only, matching the previous lowering.
pub(super) fn compile_assign(
    a: &AssignExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    match &a.left {
        Expr::Ident(i) => return compile_name_assign(a, &i.name, chunk, resolutions, compile_expr),
        Expr::Member(m) => return compile_member_assign(a, m, chunk, resolutions, compile_expr),
        Expr::Index(i) => return compile_index_assign(a, i, chunk, resolutions, compile_expr),
        _ => {}
    }
    Err(unsupported(a.pos.clone(), "assignment target"))
}

fn compile_name_assign(
    a: &AssignExpr,
    name: &str,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
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
        // Strip the `=` suffix to get the binary op (`+=` -> `+`).
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
    a: &AssignExpr,
    m: &MemberExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    compile_expr(&m.object, chunk, resolutions)?;
    if m.computed {
        compile_expr(&m.property, chunk, resolutions)?;
        compile_assign_rhs(a, chunk, resolutions, compile_expr)?;
        chunk.write_op(Opcode::SetIndex, a.pos.clone());
    } else {
        compile_assign_rhs(a, chunk, resolutions, compile_expr)?;
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
    a: &AssignExpr,
    i: &IndexExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    compile_expr(&i.left, chunk, resolutions)?;
    compile_expr(&i.index, chunk, resolutions)?;
    compile_assign_rhs(a, chunk, resolutions, compile_expr)?;
    chunk.write_op(Opcode::SetIndex, a.pos.clone());
    Ok(())
}

fn compile_assign_rhs(
    a: &AssignExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    if a.op != "=" {
        return Err(unsupported(
            a.pos.clone(),
            &format!("compound assignment `{}` to member/index target", a.op),
        ));
    }
    compile_expr(&a.right, chunk, resolutions)
}

/// Compile an update operator `++`/`--`.
pub(super) fn compile_update_operator(
    target: &Expr,
    is_prefix: bool,
    delta_op: Opcode,
    pos: Position,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    let one_idx = chunk.add_constant(crate::object::num_obj(1.0));
    match target {
        Expr::Ident(i) => {
            let name_idx = chunk.add_constant(str_obj(i.name.to_string()));
            chunk.write_op(Opcode::LoadName, pos.clone());
            chunk.write_u16(name_idx, pos.clone());
            if !is_prefix {
                chunk.write_op(Opcode::Dup, pos.clone());
            }
            chunk.write_op(Opcode::Const, pos.clone());
            chunk.write_u16(one_idx, pos.clone());
            chunk.write_op(delta_op, pos.clone());
            chunk.write_op(Opcode::AssignName, pos.clone());
            chunk.write_u16(name_idx, pos.clone());
            if !is_prefix {
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
            compile_expr(&m.object, chunk, resolutions)?;
            chunk.write_op(Opcode::Dup, pos.clone());
            let name = object_property_key_expr(&m.property);
            if name.is_empty() {
                return Err(unsupported(m.pos.clone(), "member property key"));
            }
            let name_idx = chunk.add_constant(str_obj(name));
            chunk.write_op(Opcode::GetProperty, pos.clone());
            chunk.write_u16(name_idx, pos.clone());
            chunk.write_op(Opcode::Const, pos.clone());
            chunk.write_u16(one_idx, pos.clone());
            chunk.write_op(delta_op, pos.clone());
            chunk.write_op(Opcode::SetProperty, pos.clone());
            chunk.write_u16(name_idx, pos.clone());
            Ok(())
        }
        Expr::Index(_) => Err(unsupported(
            pos.clone(),
            "++/-- on index target (assign to a temp first)",
        )),
        _ => Err(unsupported(pos.clone(), "update operator target")),
    }
}
