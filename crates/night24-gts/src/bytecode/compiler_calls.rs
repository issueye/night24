use crate::ast::{CallExpr, Expr, NewExpr, Position};
use crate::object::{str_obj, Object};

use super::chunk::Chunk;
use super::compiler::unsupported;
use super::compiler_helpers::object_property_key_expr;
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

type CompileExprFn = fn(&Expr, &mut Chunk, &ResolutionMap) -> Result<(), Object>;

pub(super) fn compile_call(
    c: &CallExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    if let Expr::Super(_) = &c.callee {
        return compile_super_constructor_call(c, chunk, resolutions, compile_expr);
    }

    let has_this_receiver = compile_call_callee(&c.callee, chunk, resolutions, compile_expr)?;
    compile_call_args(
        &c.args,
        has_this_receiver,
        c.pos.clone(),
        chunk,
        resolutions,
        compile_expr,
    )
}

fn compile_super_constructor_call(
    c: &CallExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
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
    Ok(())
}

pub(super) fn compile_new_expr(
    n: &NewExpr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
    compile_expr(&n.callee, chunk, resolutions)?;
    for arg in &n.args {
        compile_expr(arg, chunk, resolutions)?;
    }
    chunk.write_op(Opcode::New, n.pos.clone());
    chunk.write_u16(n.args.len() as u16, n.pos.clone());
    Ok(())
}

pub(super) fn compile_call_args(
    args: &[Expr],
    has_this_receiver: bool,
    pos: Position,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
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

fn compile_call_callee(
    callee: &Expr,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
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
    pos: Position,
) -> Result<u16, Object> {
    if arg_count > 0x7fff {
        return Err(unsupported(pos, "call with more than 32767 arguments"));
    }
    Ok(arg_count | if has_this_receiver { 0x8000 } else { 0 })
}
