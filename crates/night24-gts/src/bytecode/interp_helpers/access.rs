use crate::ast::Position;
use crate::object::{EnvRef, Object};

use super::super::chunk::Chunk;
use super::read_string_operand;

fn super_method_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    name: &str,
    pos: Position,
) -> Result<(), Object> {
    let value = if name == "constructor" {
        crate::evaluator::methods::get_super_constructor(env, pos)
    } else {
        crate::evaluator::methods::get_super_method(env, name, pos)
    };
    if value.is_runtime_error() {
        return Err(value);
    }
    stack.push(value);
    Ok(())
}

pub(in crate::bytecode) fn super_method_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    env: &EnvRef,
) -> Result<(), Object> {
    let (name, pos) = read_string_operand(chunk, ip, "SUPER_METHOD")?;
    super_method_stack(stack, env, &name, pos)
}
