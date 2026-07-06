use crate::ast::{ArrayLit, Expr, ObjectLit};
use crate::object::Object;

use super::chunk::Chunk;
use super::compiler::unsupported;
use super::compiler_helpers::object_property_key;
use super::emit::emit_string_operand;
use super::opcode::Opcode;
use super::resolve::ResolutionMap;

type CompileExprFn = fn(&Expr, &mut Chunk, &ResolutionMap) -> Result<(), Object>;

pub(super) fn compile_array_lit(
    a: &ArrayLit,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
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

pub(super) fn compile_object_lit(
    o: &ObjectLit,
    chunk: &mut Chunk,
    resolutions: &ResolutionMap,
    compile_expr: CompileExprFn,
) -> Result<(), Object> {
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
            emit_string_operand(chunk, Opcode::SetProperty, key, prop.pos.clone());
        }
        chunk.write_op(Opcode::Pop, prop.pos.clone());
    }
    Ok(())
}
