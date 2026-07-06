use std::rc::Rc;

use crate::object::Object;

use super::chunk::Chunk;
use super::closure::FunctionProto;
use super::compiler::{compile_stmt, FinallyFrame, LoopFrame};
use super::emit::matches_last_opcode;
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

/// Compile a function body into a sub-Chunk and return a `FunctionProto`.
///
/// The body is compiled with its own statement stream and a trailing RETURN
/// (returning the last statement's value, or Undefined). Parameters are bound
/// by the interpreter at call time into the call environment.
pub(super) fn compile_method_proto(
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
    let mut finalizers: Vec<FinallyFrame> = Vec::new();
    let n = body.statements.len();
    for (i, stmt) in body.statements.iter().enumerate() {
        compile_stmt(
            stmt,
            &mut sub,
            &mut loops,
            &mut finalizers,
            i + 1 == n,
            resolutions,
        )?;
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
pub(super) fn compile_function_proto(
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
