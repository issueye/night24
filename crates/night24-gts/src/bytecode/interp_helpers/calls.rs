use crate::ast::Position;
use crate::object::{new_error, CallContext, EnvRef, Object};

use super::super::chunk::Chunk;
use super::{read_u16_operand_with_pos, read_usize_operand_with_pos, stack_underflow};

fn push_packed_arg(
    stack: &[Object],
    value: Object,
    spread: bool,
    pos: Position,
) -> Result<(), Object> {
    let args_obj = stack.last().ok_or_else(|| stack_underflow(pos.clone()))?;
    let Object::Array(args_array) = args_obj else {
        return Err(new_error(
            pos,
            format!(
                "VMError: packed call args target is {}",
                args_obj.type_tag()
            ),
        ));
    };

    let mut args = args_array.borrow_mut();
    if spread {
        if let Object::Array(items) = value {
            args.elements
                .extend(items.borrow().elements.iter().cloned());
        } else {
            args.elements.push(value);
        }
    } else {
        args.elements.push(value);
    }
    Ok(())
}

pub(in crate::bytecode) fn call_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    env: &EnvRef,
) -> Result<(), Object> {
    let (encoded_arg_count, pos) = read_u16_operand_with_pos(chunk, ip, "CALL")?;
    let has_this_receiver = encoded_arg_count & 0x8000 != 0;
    let arg_count = (encoded_arg_count & 0x7fff) as usize;
    call_stack(stack, env, arg_count, has_this_receiver, pos)
}

pub(in crate::bytecode) fn push_arg_stack(
    stack: &mut Vec<Object>,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    push_packed_arg(stack, value, false, pos)
}

pub(in crate::bytecode) fn spread_stack(
    stack: &mut Vec<Object>,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let target = stack
        .last()
        .ok_or_else(|| stack_underflow(pos.clone()))?
        .clone();
    match target {
        Object::Array(_) => push_packed_arg(stack, value, true, pos),
        Object::Hash(hash) => {
            if let Object::Hash(source) = value {
                for (key, copied) in source.borrow().entries.iter() {
                    hash.borrow_mut().set(key.clone(), copied.clone());
                }
            }
            Ok(())
        }
        other => Err(new_error(
            pos,
            format!("VMError: SPREAD target is {}", other.type_tag()),
        )),
    }
}

fn call_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    arg_count: usize,
    has_this_receiver: bool,
    pos: Position,
) -> Result<(), Object> {
    // Stack: [..., receiver?, callee, arg1, ..., argN].
    let stack_len = stack.len();
    let needed = arg_count + 1 + usize::from(has_this_receiver);
    if stack_len < needed {
        return Err(stack_underflow(pos));
    }

    let args: Vec<Object> = stack.split_off(stack_len - arg_count);
    let callee = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let this = if has_this_receiver {
        Some(stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?)
    } else {
        None
    };
    let result = call_value(env, callee, &args, this, pos)?;
    stack.push(result);
    Ok(())
}

pub(in crate::bytecode) fn call_spread_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    pos: Position,
) -> Result<(), Object> {
    let args_obj = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let callee = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let args = match args_obj {
        Object::Array(a) => a.borrow().elements.clone(),
        other => {
            return Err(new_error(
                pos,
                format!(
                    "VMError: CALL_SPREAD expected args array, got {}",
                    other.type_tag()
                ),
            ));
        }
    };
    let result = call_value(env, callee, &args, None, pos)?;
    stack.push(result);
    Ok(())
}

fn construct_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    arg_count: usize,
    pos: Position,
) -> Result<(), Object> {
    let stack_len = stack.len();
    if stack_len < arg_count + 1 {
        return Err(stack_underflow(pos));
    }
    let args: Vec<Object> = stack.split_off(stack_len - arg_count);
    let callee = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let result = construct_value(env, callee, &args, pos)?;
    stack.push(result);
    Ok(())
}

pub(in crate::bytecode) fn construct_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    env: &EnvRef,
) -> Result<(), Object> {
    let (arg_count, pos) = read_usize_operand_with_pos(chunk, ip, "NEW")?;
    construct_stack(stack, env, arg_count, pos)
}

fn call_value(
    env: &EnvRef,
    callee: Object,
    args: &[Object],
    this: Option<Object>,
    pos: Position,
) -> Result<Object, Object> {
    match callee {
        Object::Builtin(b) => {
            let mut ctx = CallContext::new(env, pos);
            ctx.receiver = b.extra.clone().or(this);
            let result = (b.func)(&mut ctx, args);
            if result.is_runtime_error() {
                Err(result)
            } else {
                Ok(result)
            }
        }
        Object::Closure(c) => {
            crate::bytecode::call::call_closure_with_this(&c, args, env, this, pos)
        }
        // Tree-walker functions are still callable (e.g. globals installed by
        // register_globals that are Function values). Delegate to the shared
        // apply_function so semantics stay identical.
        other @ (Object::Function(_) | Object::Class(_)) => {
            let r = crate::evaluator::expressions::apply_function(&other, env, args, this, pos);
            if r.is_runtime_error() {
                Err(r)
            } else {
                Ok(r)
            }
        }
        Object::Hash(h) => {
            if let Some(Object::Builtin(b)) = h.borrow().get("__call").cloned() {
                let mut ctx = CallContext::new(env, pos);
                ctx.receiver = b.extra.clone().or(this);
                let result = (b.func)(&mut ctx, args);
                if result.is_runtime_error() {
                    Err(result)
                } else {
                    Ok(result)
                }
            } else {
                Err(new_error(pos, "TypeError: object is not callable"))
            }
        }
        _ => Err(new_error(
            pos,
            format!("TypeError: {} is not callable", callee.type_tag()),
        )),
    }
}

fn construct_value(
    env: &EnvRef,
    callee: Object,
    args: &[Object],
    pos: Position,
) -> Result<Object, Object> {
    let result = match callee {
        Object::Class(cls) => {
            crate::evaluator::methods::construct_class(&cls, env, args, pos.clone())
        }
        Object::Builtin(b) => {
            crate::evaluator::methods::construct_builtin(&b, env, args, pos.clone())
        }
        Object::Function(f) => crate::evaluator::expressions::apply_function(
            &Object::Function(f),
            env,
            args,
            None,
            pos.clone(),
        ),
        Object::Hash(_) => {
            crate::evaluator::expressions::apply_function(&callee, env, args, None, pos.clone())
        }
        other => {
            return Err(new_error(
                pos,
                format!("TypeError: {} is not a constructor", other.type_tag()),
            ));
        }
    };
    if result.is_runtime_error() {
        Err(result)
    } else {
        Ok(result)
    }
}
