use crate::ast::Position;
use crate::object::{new_error, str_obj, ArrayData, EnvRef, HashData, Object};
use std::cell::RefCell;
use std::rc::Rc;

use super::super::chunk::Chunk;
use super::{read_string_operand, read_usize_operand_with_pos, stack_underflow};

pub(in crate::bytecode) fn array_slice_from_stack(
    stack: &mut Vec<Object>,
    pos: Position,
) -> Result<(), Object> {
    let start_val = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let start = match start_val {
        Object::Number(n) => n as usize,
        other => {
            return Err(new_error(
                pos.clone(),
                format!(
                    "TypeError: array slice start must be a number, got {}",
                    other.type_tag()
                ),
            ));
        }
    };
    let array_val = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let tail: Vec<Object> = match array_val {
        Object::Array(arr) => {
            let arr = arr.borrow();
            arr.elements[start.min(arr.elements.len())..].to_vec()
        }
        other => {
            // Non-array source: empty tail (parity with tree-walker).
            let _ = other;
            Vec::new()
        }
    };
    stack.push(Object::Array(Rc::new(RefCell::new(ArrayData {
        elements: tail,
    }))));
    Ok(())
}

fn new_array_from_stack(
    stack: &mut Vec<Object>,
    count: usize,
    pos: Position,
) -> Result<(), Object> {
    if stack.len() < count {
        return Err(stack_underflow(pos));
    }
    let elements = stack.split_off(stack.len() - count);
    stack.push(Object::Array(Rc::new(RefCell::new(ArrayData { elements }))));
    Ok(())
}

pub(in crate::bytecode) fn new_array_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
) -> Result<(), Object> {
    let (count, pos) = read_usize_operand_with_pos(chunk, ip, "NEW_ARRAY")?;
    new_array_from_stack(stack, count, pos)
}

pub(in crate::bytecode) fn new_object_to_stack(stack: &mut Vec<Object>) {
    stack.push(Object::Hash(Rc::new(RefCell::new(HashData::default()))));
}

fn set_property_stack(stack: &mut Vec<Object>, name: &str, pos: Position) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let obj = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let assigned = assign_property(&obj, name, value, pos)?;
    stack.push(assigned);
    Ok(())
}

pub(in crate::bytecode) fn set_property_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
) -> Result<(), Object> {
    let (name, pos) = read_string_operand(chunk, ip, "SET_PROPERTY")?;
    set_property_stack(stack, &name, pos)
}

fn get_property_stack(stack: &mut Vec<Object>, name: &str, pos: Position) -> Result<(), Object> {
    let obj = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let value = crate::evaluator::methods::get_property(&obj, name, pos);
    if value.is_runtime_error() {
        return Err(value);
    }
    stack.push(value);
    Ok(())
}

pub(in crate::bytecode) fn get_property_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
) -> Result<(), Object> {
    let (name, pos) = read_string_operand(chunk, ip, "GET_PROPERTY")?;
    get_property_stack(stack, &name, pos)
}

pub(in crate::bytecode) fn get_index_stack(
    stack: &mut Vec<Object>,
    pos: Position,
) -> Result<(), Object> {
    let key = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let obj = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let value = crate::evaluator::methods::get_index(&obj, &key, pos);
    if value.is_runtime_error() {
        return Err(value);
    }
    stack.push(value);
    Ok(())
}

pub(in crate::bytecode) fn set_index_stack(
    stack: &mut Vec<Object>,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let key = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let obj = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let assigned = assign_index(&obj, &key, value, pos)?;
    stack.push(assigned);
    Ok(())
}

pub(in crate::bytecode) fn iter_keys_stack(
    stack: &mut Vec<Object>,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let elements = crate::evaluator::eval_core::iterable_keys(&value)
        .into_iter()
        .map(str_obj)
        .collect();
    stack.push(Object::Array(Rc::new(RefCell::new(ArrayData { elements }))));
    Ok(())
}

pub(in crate::bytecode) fn iter_values_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let iterator = crate::evaluator::iterator::get_iterator(&value, env, pos);
    if iterator.is_runtime_error() {
        return Err(iterator);
    }
    stack.push(iterator);
    Ok(())
}

pub(in crate::bytecode) fn iter_next_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    pos: Position,
) -> Result<(), Object> {
    let iterator = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let next = crate::evaluator::iterator::iterator_next(&iterator, env, pos);
    if next.is_runtime_error() {
        return Err(next);
    }
    stack.push(next);
    Ok(())
}

pub(in crate::bytecode) fn len_stack(stack: &mut Vec<Object>, pos: Position) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let len = match &value {
        Object::Array(a) => a.borrow().elements.len(),
        Object::Hash(h) => h.borrow().entries.len(),
        Object::String(s) => s.chars().count(),
        Object::Map(m) => m.borrow().size(),
        Object::Set(s) => s.borrow().size(),
        _ => {
            return Err(new_error(
                pos,
                format!("TypeError: cannot get length of {}", value.type_tag()),
            ));
        }
    };
    stack.push(Object::Number(len as f64));
    Ok(())
}

fn assign_property(
    obj: &Object,
    name: &str,
    value: Object,
    pos: Position,
) -> Result<Object, Object> {
    match obj {
        Object::Hash(h) => {
            if h.borrow().frozen {
                return Err(new_error(pos, "TypeError: cannot assign to frozen object"));
            }
            if h.borrow().sealed && !h.borrow().contains(name) {
                return Err(new_error(
                    pos,
                    "TypeError: cannot add property to sealed object",
                ));
            }
            h.borrow_mut().set(name, value.clone());
            Ok(value)
        }
        Object::Instance(i) => {
            i.borrow_mut().props.insert(name.into(), value.clone());
            Ok(value)
        }
        Object::Class(c) => {
            c.borrow_mut().statics.insert(name.into(), value.clone());
            Ok(value)
        }
        _ => Err(new_error(
            pos,
            format!("TypeError: cannot assign to property of {}", obj.type_tag()),
        )),
    }
}

fn assign_index(
    obj: &Object,
    key: &Object,
    value: Object,
    pos: Position,
) -> Result<Object, Object> {
    match obj {
        Object::Array(a) => {
            if let Object::Number(n) = key {
                let i = *n as isize;
                let mut arr = a.borrow_mut();
                let len = arr.elements.len() as isize;
                if i < 0 || i >= len {
                    return Err(new_error(pos, "RangeError: array index out of bounds"));
                }
                arr.elements[i as usize] = value.clone();
            }
            Ok(value)
        }
        Object::Hash(h) => {
            if h.borrow().frozen {
                return Err(new_error(pos, "TypeError: cannot assign to frozen object"));
            }
            h.borrow_mut().set(key.inspect(), value.clone());
            Ok(value)
        }
        _ => Err(new_error(
            pos,
            format!("TypeError: cannot index {}", obj.type_tag()),
        )),
    }
}
