use crate::ast::Position;
use crate::object::str_obj;

use super::chunk::Chunk;
use super::opcode::Opcode;

pub(super) fn decl_name_operand(chunk: &mut Chunk, name: &str, is_const: bool) -> u16 {
    let name_idx = chunk.add_constant(str_obj(name.to_string()));
    // Encode const-ness in the high bit of the name index operand so the
    // interpreter knows which binding flavor to create. (Name pools stay
    // small; a u16 with a flag bit is plenty.)
    if is_const {
        name_idx | 0x8000
    } else {
        name_idx
    }
}

pub(super) fn emit_decl_store(chunk: &mut Chunk, name: &str, is_const: bool, pos: Position) {
    let operand = decl_name_operand(chunk, name, is_const);
    chunk.write_op(Opcode::StoreName, pos.clone());
    chunk.write_u16(operand, pos);
}
