use crate::ast::{Position, TypeAnnotation};
use crate::object::{new_error, Object};

use super::super::chunk::Chunk;

fn read_byte_operand(chunk: &Chunk, ip: &mut usize, opcode: &'static str) -> Result<u8, Object> {
    let Some(byte) = chunk.code.get(*ip).copied() else {
        return Err(new_error(
            chunk.position_at(ip.saturating_sub(1)),
            format!("VMError: {} missing operand", opcode),
        ));
    };
    *ip += 1;
    Ok(byte)
}

pub(in crate::bytecode) fn read_byte_operand_with_pos(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<(usize, Position), Object> {
    let value = read_byte_operand(chunk, ip, opcode)? as usize;
    let pos = chunk.position_at(*ip - 2);
    Ok((value, pos))
}

fn read_u16_operand(chunk: &Chunk, ip: &mut usize, opcode: &'static str) -> Result<u16, Object> {
    let Some(bytes) = chunk.code.get(*ip..ip.saturating_add(2)) else {
        return Err(new_error(
            chunk.position_at(ip.saturating_sub(1)),
            format!("VMError: {} missing operand", opcode),
        ));
    };
    let value = ((bytes[0] as u16) << 8) | bytes[1] as u16;
    *ip += 2;
    Ok(value)
}

pub(in crate::bytecode) fn read_u16_operand_with_pos(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<(u16, Position), Object> {
    let value = read_u16_operand(chunk, ip, opcode)?;
    let pos = chunk.position_at(*ip - 3);
    Ok((value, pos))
}

pub(in crate::bytecode) fn read_usize_operand_with_pos(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<(usize, Position), Object> {
    let (value, pos) = read_u16_operand_with_pos(chunk, ip, opcode)?;
    Ok((value as usize, pos))
}

pub(in crate::bytecode) fn read_u32_operand(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<u32, Object> {
    let Some(bytes) = chunk.code.get(*ip..ip.saturating_add(4)) else {
        return Err(new_error(
            chunk.position_at(ip.saturating_sub(1)),
            format!("VMError: {} missing operand", opcode),
        ));
    };
    let value = ((bytes[0] as u32) << 24)
        | ((bytes[1] as u32) << 16)
        | ((bytes[2] as u32) << 8)
        | bytes[3] as u32;
    *ip += 4;
    Ok(value)
}

pub(in crate::bytecode) fn read_u32_operand_with_pos(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<(u32, Position), Object> {
    let value = read_u32_operand(chunk, ip, opcode)?;
    let pos = chunk.position_at(*ip - 5);
    Ok((value, pos))
}

pub(in crate::bytecode) fn read_string_operand(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<(String, Position), Object> {
    let idx = read_u16_operand(chunk, ip, opcode)? as usize;
    let pos = chunk.position_at(*ip - 3);
    let value = read_string_const(&chunk.constants, idx, pos.clone(), opcode)?;
    Ok((value, pos))
}

pub(in crate::bytecode) fn read_name_operand(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<(String, bool, Position), Object> {
    let operand = read_u16_operand(chunk, ip, opcode)?;
    let pos = chunk.position_at(*ip - 3);
    let is_const = operand & 0x8000 != 0;
    let name_idx = (operand & 0x7fff) as usize;
    let name = read_string_const(&chunk.constants, name_idx, pos.clone(), opcode)?;
    Ok((name, is_const, pos))
}

fn read_const_operand(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<Object, Object> {
    let idx = read_u16_operand(chunk, ip, opcode)? as usize;
    let pos = chunk.position_at(*ip - 3);
    chunk.constants.get(idx).cloned().ok_or_else(|| {
        new_error(
            pos,
            format!("VMError: {} constant index {} out of range", opcode, idx),
        )
    })
}

pub(in crate::bytecode) fn push_const_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
) -> Result<(), Object> {
    stack.push(read_const_operand(chunk, ip, "CONST")?);
    Ok(())
}

pub(in crate::bytecode) fn read_type_operand(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
    pos: Position,
) -> Result<TypeAnnotation, Object> {
    let type_idx = read_u16_operand(chunk, ip, opcode)? as usize;
    chunk.types.get(type_idx).cloned().ok_or_else(|| {
        new_error(
            pos,
            format!("VMError: missing type annotation {}", type_idx),
        )
    })
}

fn read_string_const(
    constants: &[Object],
    idx: usize,
    pos: Position,
    opcode: &'static str,
) -> Result<String, Object> {
    match constants.get(idx) {
        Some(Object::String(s)) => Ok(s.to_string()),
        _ => Err(new_error(
            pos,
            format!("VMError: {} operand is not a string", opcode),
        )),
    }
}
