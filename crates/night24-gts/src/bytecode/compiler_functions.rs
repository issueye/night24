use crate::ast::{ArrowBody, ArrowFuncExpr, BlockStmt, FuncDecl, FuncExpr, ReturnStmt, Stmt};
use crate::object::Object;

use super::chunk::Chunk;
use super::compiler_function_proto::compile_function_proto;
use super::emit::emit_string_operand;
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

pub(super) fn compile_func_expr(
    f: &FuncExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
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

pub(super) fn compile_func_decl(
    f: &FuncDecl,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    // Compile the body to a proto (which lives in this chunk's proto table),
    // emit OP_CLOSURE to construct the closure value, then store it under the
    // function's name.
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
    emit_string_operand(chunk, Opcode::StoreName, f.name.clone(), f.pos.clone());
    Ok(())
}

pub(super) fn compile_arrow_expr(
    a: &ArrowFuncExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
) -> Result<(), Object> {
    let body = match &a.body {
        ArrowBody::Expr(e) => BlockStmt {
            pos: a.pos.clone(),
            statements: vec![Stmt::Return(ReturnStmt {
                pos: a.pos.clone(),
                value: Some(e.clone()),
            })],
        },
        ArrowBody::Block(b) => b.clone(),
    };
    let idx = compile_function_proto(
        "",
        a.params.clone(),
        body,
        a.is_async,
        true,
        a.return_t.clone(),
        a.pos.clone(),
        chunk,
        resolutions,
    )?;
    chunk.write_op(Opcode::Closure, a.pos.clone());
    chunk.write_u16(idx, a.pos.clone());
    Ok(())
}
