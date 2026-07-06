use crate::ast::Position;
use crate::object::{str_obj, Object};

use super::chunk::Chunk;
use super::opcode::Opcode;

pub(super) fn emit_load_name(chunk: &mut Chunk, name: &str, pos: Position) {
    let idx = chunk.add_constant(str_obj(name.to_string()));
    chunk.write_op(Opcode::LoadName, pos.clone());
    chunk.write_u16(idx, pos);
}

pub(super) fn patch_jump_to(chunk: &mut Chunk, operand_ip: u32, target: u32) {
    let ip = operand_ip as usize;
    let bytes = target.to_be_bytes();
    chunk.code[ip] = bytes[0];
    chunk.code[ip + 1] = bytes[1];
    chunk.code[ip + 2] = bytes[2];
    chunk.code[ip + 3] = bytes[3];
}

/// Emit `<op> <placeholder u32>` and return the byte offset of the placeholder
/// so the caller can patch it with `patch_jump_here`.
pub(super) fn emit_jump_placeholder(chunk: &mut Chunk, op: Opcode, pos: Position) -> u32 {
    chunk.write_op(op, pos.clone());
    let patch = chunk.code.len() as u32;
    chunk.write_u32(0, pos);
    patch
}

/// Patch a jump placeholder to point at the current code position.
pub(super) fn patch_jump_here(chunk: &mut Chunk, operand_ip: u32) {
    let target = chunk.code.len() as u32;
    patch_jump_to(chunk, operand_ip, target);
}

pub(super) fn emit_const(chunk: &mut Chunk, idx: u16, pos: Position) {
    chunk.write_op(Opcode::Const, pos.clone());
    chunk.write_u16(idx, pos);
}

/// Intern a compile-time-known value into the constant pool and emit a `Const`
/// instruction that loads it.
pub(super) fn emit_value_constant(
    chunk: &mut Chunk,
    value: Object,
    pos: Position,
) -> Result<(), Object> {
    let idx = chunk.add_constant(value);
    emit_const(chunk, idx, pos);
    Ok(())
}

pub(super) fn matches_last_opcode(chunk: &Chunk, op: Opcode) -> bool {
    let mut ip = 0;
    let mut last_op = None;
    while ip < chunk.code.len() {
        let byte = chunk.code[ip];
        last_op = Opcode::from_byte(byte);
        ip += 1;
        if let Some(opcode) = last_op {
            ip += operand_width(opcode) as usize;
        }
    }
    last_op == Some(op)
}

pub(super) fn operand_width(op: Opcode) -> u8 {
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
