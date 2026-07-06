use crate::ast::Position;
use crate::object::{new_error, new_named_error, Object};
use std::cell::RefCell;
use std::rc::Rc;

use super::super::chunk::Chunk;
use super::{read_u32_operand, read_u32_operand_with_pos, stack_underflow};

pub(in crate::bytecode) fn jump_to_operand(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<(), Object> {
    let target = read_u32_operand(chunk, ip, opcode)? as usize;
    *ip = target;
    Ok(())
}

pub(in crate::bytecode) fn conditional_jump_from_stack(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    opcode: &'static str,
    jump_when_truthy: bool,
) -> Result<(), Object> {
    let (target, pos) = read_u32_operand_with_pos(chunk, ip, opcode)?;
    let cond = stack.pop().ok_or_else(|| stack_underflow(pos))?;
    if cond.is_truthy() == jump_when_truthy {
        *ip = target as usize;
    }
    Ok(())
}

fn throw_value(value: Object, pos: Position) -> Object {
    match value {
        Object::Error(e) => {
            let mut data = e.borrow_mut().clone();
            data.runtime = true;
            if data.pos.is_zero() {
                data.pos = pos.clone();
            }
            if data.stack.is_empty() {
                data.stack = if pos.is_zero() {
                    format!("{}: {}", data.name, data.message)
                } else {
                    format!("{}: {}\n    at {}", data.name, data.message, pos)
                };
            }
            Object::Error(Rc::new(RefCell::new(data)))
        }
        other => {
            let err = new_named_error(pos, "Error", other.inspect());
            if let Object::Error(data) = &err {
                data.borrow_mut().thrown = Some(other);
            }
            err
        }
    }
}

pub(in crate::bytecode) fn throw_from_stack(
    stack: &mut Vec<Object>,
    pos: Position,
) -> Result<Object, Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    Err(throw_value(value, pos))
}

fn catch_value(value: Object) -> Object {
    match value {
        Object::Error(e) => {
            let mut data = e.borrow_mut().clone();
            data.runtime = false;
            Object::Error(Rc::new(RefCell::new(data)))
        }
        other => other,
    }
}

pub(in crate::bytecode) fn unwind_to_handler(
    chunk: &Chunk,
    last_ip: usize,
    stack: &mut Vec<Object>,
    error: Object,
) -> Option<usize> {
    let region = chunk
        .protected_regions
        .iter()
        .filter(|region| {
            let fault_ip = last_ip as u32;
            region.try_start <= fault_ip
                && fault_ip < region.try_end
                && region.handler_ip > region.try_end
        })
        .max_by_key(|region| region.try_start)?;

    stack.push(catch_value(error));
    Some(region.handler_ip as usize)
}

pub(in crate::bytecode) fn throw_match_error_from_stack(
    stack: &mut Vec<Object>,
    pos: Position,
) -> Result<Object, Object> {
    let subject = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    Err(new_error(
        pos,
        format!("MatchError: no arm matched for {}", subject.inspect()),
    ))
}
