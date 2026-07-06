use crate::ast::ClassDecl;
use crate::object::{str_obj, Object};

use super::chunk::Chunk;
use super::compiler_helpers::add_class_decl;
use super::opcode::Opcode;

pub(super) fn compile_class_decl(c: &ClassDecl, chunk: &mut Chunk) -> Result<(), Object> {
    emit_class_value(c, chunk);
    let name_idx = chunk.add_constant(str_obj(c.name.clone()));
    chunk.write_op(Opcode::StoreName, c.pos.clone());
    chunk.write_u16(name_idx, c.pos.clone());
    Ok(())
}

pub(super) fn compile_class_expr(c: &ClassDecl, chunk: &mut Chunk) -> Result<(), Object> {
    emit_class_value(c, chunk);
    Ok(())
}

fn emit_class_value(c: &ClassDecl, chunk: &mut Chunk) {
    let class_idx = add_class_decl(chunk, c.clone());
    chunk.write_op(Opcode::NewClass, c.pos.clone());
    chunk.write_u16(class_idx, c.pos.clone());
}
