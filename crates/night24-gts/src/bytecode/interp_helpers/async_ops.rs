use crate::ast::Position;
use crate::object::{new_error, EnvRef, Object, PromiseState};
use std::cell::RefCell;
use std::rc::Rc;

use super::stack_underflow;

pub(in crate::bytecode) fn await_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let awaited = await_value(value, env, pos)?;
    stack.push(awaited);
    Ok(())
}

fn await_value(value: Object, env: &EnvRef, pos: Position) -> Result<Object, Object> {
    match &value {
        Object::Promise(promise) => {
            if promise.state() == PromiseState::Pending {
                env.borrow().vm.wait_async();
            }
            let result = promise.wait();
            if promise.state() == PromiseState::Rejected {
                return Err(match &result {
                    Object::Error(data) => {
                        let mut error = data.borrow().clone();
                        error.runtime = true;
                        if error.pos.is_zero() {
                            error.pos = pos;
                        }
                        Object::Error(Rc::new(RefCell::new(error)))
                    }
                    other => new_error(pos, other.inspect()),
                });
            }
            Ok(result)
        }
        _ => Ok(value),
    }
}
