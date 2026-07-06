use crate::ast::Position;
use crate::object::{new_error, str_obj, Object};

pub(in crate::bytecode) fn stack_underflow(pos: Position) -> Object {
    new_error(pos, "VMError: stack underflow")
}

pub(in crate::bytecode) fn dup_stack(stack: &mut Vec<Object>, pos: Position) -> Result<(), Object> {
    let value = stack.last().cloned().ok_or_else(|| stack_underflow(pos))?;
    stack.push(value);
    Ok(())
}

pub(in crate::bytecode) fn apply_binary_stack_op(
    stack: &mut Vec<Object>,
    op: &'static str,
    pos: Position,
) -> Result<(), Object> {
    let right = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let left = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let result = crate::evaluator::expressions::apply_binary_op(op, &left, &right, pos);
    if result.is_runtime_error() {
        return Err(result);
    }
    stack.push(result);
    Ok(())
}

pub(in crate::bytecode) fn apply_unary_stack_op(
    stack: &mut Vec<Object>,
    op: &'static str,
    pos: Position,
) -> Result<(), Object> {
    let right = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let result = crate::evaluator::expressions::apply_unary_op(op, &right, pos);
    if result.is_runtime_error() {
        return Err(result);
    }
    stack.push(result);
    Ok(())
}

pub(in crate::bytecode) fn to_string_stack(
    stack: &mut Vec<Object>,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos))?;
    // Mirror the tree-walker's template interpolation (inspect()).
    stack.push(str_obj(value.inspect()));
    Ok(())
}

pub(in crate::bytecode) fn type_of_stack(
    stack: &mut Vec<Object>,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos))?;
    stack.push(str_obj(crate::evaluator::expressions::typeof_name(&value)));
    Ok(())
}
