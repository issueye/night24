use crate::ast::{Position, TypeAnnotation, TypeKind};
use crate::object::{
    new_error, new_named_error, str_obj, ArrayData, CallContext, EnvRef, HashData, Object,
    PromiseState,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::Ordering;

use super::chunk::Chunk;
use super::closure::FunctionProto;
use super::upvalue::Upvalue;

pub(super) fn read_byte_operand(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<u8, Object> {
    let Some(byte) = chunk.code.get(*ip).copied() else {
        return Err(new_error(
            chunk.position_at(ip.saturating_sub(1)),
            format!("VMError: {} missing operand", opcode),
        ));
    };
    *ip += 1;
    Ok(byte)
}

pub(super) fn read_byte_operand_with_pos(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<(usize, Position), Object> {
    let value = read_byte_operand(chunk, ip, opcode)? as usize;
    let pos = chunk.position_at(*ip - 2);
    Ok((value, pos))
}

pub(super) fn read_u16_operand(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<u16, Object> {
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

pub(super) fn read_u16_operand_with_pos(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<(u16, Position), Object> {
    let value = read_u16_operand(chunk, ip, opcode)?;
    let pos = chunk.position_at(*ip - 3);
    Ok((value, pos))
}

pub(super) fn read_usize_operand_with_pos(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<(usize, Position), Object> {
    let (value, pos) = read_u16_operand_with_pos(chunk, ip, opcode)?;
    Ok((value as usize, pos))
}

pub(super) fn read_u32_operand(
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

pub(super) fn read_u32_operand_with_pos(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<(u32, Position), Object> {
    let value = read_u32_operand(chunk, ip, opcode)?;
    let pos = chunk.position_at(*ip - 5);
    Ok((value, pos))
}

pub(super) fn read_string_operand(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<(String, Position), Object> {
    let idx = read_u16_operand(chunk, ip, opcode)? as usize;
    let pos = chunk.position_at(*ip - 3);
    let value = read_string_const(&chunk.constants, idx, pos.clone(), opcode)?;
    Ok((value, pos))
}

pub(super) fn read_name_operand(
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

pub(super) fn read_const_operand(
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

pub(super) fn read_type_operand(
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

pub(super) fn throw_value(value: Object, pos: Position) -> Object {
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

pub(super) fn catch_value(value: Object) -> Object {
    match value {
        Object::Error(e) => {
            let mut data = e.borrow_mut().clone();
            data.runtime = false;
            Object::Error(Rc::new(RefCell::new(data)))
        }
        other => other,
    }
}

pub(super) fn unwind_to_handler(
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

pub(super) fn stack_underflow(pos: Position) -> Object {
    new_error(pos, "VMError: stack underflow")
}

pub(super) fn apply_binary_stack_op(
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

pub(super) fn apply_unary_stack_op(
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

pub(super) fn to_string_stack(stack: &mut Vec<Object>, pos: Position) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos))?;
    // Mirror the tree-walker's template interpolation (inspect()).
    stack.push(str_obj(value.inspect()));
    Ok(())
}

pub(super) fn type_of_stack(stack: &mut Vec<Object>, pos: Position) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos))?;
    stack.push(str_obj(crate::evaluator::expressions::typeof_name(&value)));
    Ok(())
}

pub(super) fn await_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let awaited = await_value(value, env, pos)?;
    stack.push(awaited);
    Ok(())
}

pub(super) fn throw_match_error_from_stack(
    stack: &mut Vec<Object>,
    pos: Position,
) -> Result<Object, Object> {
    let subject = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    Err(new_error(
        pos,
        format!("MatchError: no arm matched for {}", subject.inspect()),
    ))
}

pub(super) fn close_open_upvalues_from(
    open_upvalues: &mut BTreeMap<usize, Vec<Rc<Upvalue>>>,
    stack: &[Object],
    first_slot: usize,
) {
    let closing_slots: Vec<usize> = open_upvalues
        .range(first_slot..)
        .map(|(slot, _)| *slot)
        .collect();
    for slot in closing_slots {
        if let Some(upvalues) = open_upvalues.remove(&slot) {
            for upvalue in upvalues {
                upvalue.close_from_slots(stack);
            }
        }
    }
}

pub(super) fn load_local(
    stack: &mut Vec<Object>,
    slot: usize,
    pos: Position,
) -> Result<(), Object> {
    let Some(value) = stack.get(slot).cloned() else {
        return Err(new_error(
            pos,
            format!("VMError: LOAD_LOCAL slot {} out of range", slot),
        ));
    };
    stack.push(value);
    Ok(())
}

pub(super) fn store_local(
    stack: &mut Vec<Object>,
    slot: usize,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let Some(target) = stack.get_mut(slot) else {
        return Err(new_error(
            pos,
            format!("VMError: STORE_LOCAL slot {} out of range", slot),
        ));
    };
    *target = value;
    Ok(())
}

pub(super) fn load_upvalue(
    stack: &mut Vec<Object>,
    upvalues: &[Rc<Upvalue>],
    index: usize,
    pos: Position,
) -> Result<(), Object> {
    let Some(upvalue) = upvalues.get(index) else {
        return Err(new_error(
            pos,
            format!("VMError: missing upvalue {}", index),
        ));
    };
    let Some(value) = upvalue.get(stack) else {
        return Err(new_error(
            pos,
            format!("VMError: open upvalue {} points outside stack", index),
        ));
    };
    stack.push(value);
    Ok(())
}

pub(super) fn store_upvalue(
    stack: &mut Vec<Object>,
    upvalues: &[Rc<Upvalue>],
    index: usize,
    pos: Position,
) -> Result<(), Object> {
    let value = stack
        .last()
        .cloned()
        .ok_or_else(|| stack_underflow(pos.clone()))?;
    let Some(upvalue) = upvalues.get(index) else {
        return Err(new_error(
            pos,
            format!("VMError: missing upvalue {}", index),
        ));
    };
    if !upvalue.set(stack, value) {
        return Err(new_error(
            pos,
            format!("VMError: upvalue {} points outside stack", index),
        ));
    }
    Ok(())
}

pub(super) fn load_name(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    name: &str,
    pos: Position,
) -> Result<(), Object> {
    let value = match env.borrow().get(name) {
        Some(v) => v,
        None => {
            return Err(new_error(
                pos,
                format!("ReferenceError: '{}' is not defined", name),
            ));
        }
    };
    stack.push(value);
    Ok(())
}

pub(super) fn store_name(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    name: String,
    is_const: bool,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos))?;
    if is_const {
        env.borrow_mut().set_const_here(name, value);
    } else {
        env.borrow_mut().set_here(name, value);
    }
    Ok(())
}

pub(super) fn store_typed_name(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    name: String,
    is_const: bool,
    type_anno: TypeAnnotation,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    check_type_annotation(env, &name, &value, &type_anno, pos)?;
    if is_const {
        env.borrow_mut()
            .set_typed_const(name, value, Some(type_anno));
    } else {
        env.borrow_mut().set_typed(name, value, Some(type_anno));
    }
    Ok(())
}

pub(super) fn assign_name(
    stack: &mut [Object],
    env: &EnvRef,
    name: &str,
    pos: Position,
) -> Result<(), Object> {
    let value = stack
        .last()
        .cloned()
        .ok_or_else(|| stack_underflow(pos.clone()))?;
    let Some((is_const, type_anno)) = binding_info(env, name) else {
        return Err(new_error(
            pos,
            format!("ReferenceError: '{}' is not defined", name),
        ));
    };
    if is_const {
        return Err(new_error(
            pos,
            format!("TypeError: assignment to constant '{}'", name),
        ));
    }
    if let Some(type_anno) = type_anno {
        check_type_annotation(env, name, &value, &type_anno, pos)?;
    }
    let (found, is_const) = env.borrow_mut().assign(name, value);
    debug_assert!(found);
    debug_assert!(!is_const);
    Ok(())
}

pub(super) fn load_global(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    name: &str,
    pos: Position,
) -> Result<(), Object> {
    match env.borrow().vm.get_global(name) {
        Some(v) => {
            stack.push(v);
            Ok(())
        }
        None => Err(new_error(
            pos,
            format!("ReferenceError: '{}' is not defined", name),
        )),
    }
}

pub(super) fn store_global(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    name: String,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos))?;
    env.borrow().vm.set_global(name, value);
    Ok(())
}

pub(super) fn import_module_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    source: &str,
    pos: Position,
) -> Result<(), Object> {
    let importer = env.borrow().vm.importer();
    let module = match importer {
        Some(importer) => importer(env, source)?,
        None => {
            return Err(new_error(
                pos,
                "ImportError: module loading is not configured",
            ));
        }
    };
    stack.push(module);
    Ok(())
}

pub(super) fn export_name_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    name: String,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let exports = env.borrow().get("exports").unwrap_or(Object::Undefined);
    match exports {
        Object::Hash(h) => {
            h.borrow_mut().set(name, value);
            Ok(())
        }
        other => Err(new_error(
            pos,
            format!("TypeError: cannot export from {}", other.type_tag()),
        )),
    }
}

pub(super) fn export_all_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    pos: Position,
) -> Result<(), Object> {
    let source_exports = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let current_exports = env.borrow().get("exports").unwrap_or(Object::Undefined);
    match (&source_exports, &current_exports) {
        (Object::Hash(src), Object::Hash(dst)) => {
            let pairs: Vec<(String, Object)> = {
                let sb = src.borrow();
                sb.entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            };
            for (k, v) in pairs {
                // `export *` does NOT re-export a `default` binding.
                if k == "default" {
                    continue;
                }
                dst.borrow_mut().set(k, v);
            }
            Ok(())
        }
        (other_src, _) => Err(new_error(
            pos,
            format!(
                "TypeError: export * source must be a module object, got {}",
                other_src.type_tag()
            ),
        )),
    }
}

pub(super) fn wrap_resolved_promise_stack(
    stack: &mut Vec<Object>,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos))?;
    let promise = crate::object::Promise::new();
    promise.resolve(value);
    stack.push(Object::Promise(promise));
    Ok(())
}

pub(super) fn array_slice_from_stack(stack: &mut Vec<Object>, pos: Position) -> Result<(), Object> {
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

pub(super) fn new_array_from_stack(
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

pub(super) fn new_object_to_stack(stack: &mut Vec<Object>) {
    stack.push(Object::Hash(Rc::new(RefCell::new(HashData::default()))));
}

pub(super) fn set_property_stack(
    stack: &mut Vec<Object>,
    name: &str,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let obj = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let assigned = assign_property(&obj, name, value, pos)?;
    stack.push(assigned);
    Ok(())
}

pub(super) fn get_property_stack(
    stack: &mut Vec<Object>,
    name: &str,
    pos: Position,
) -> Result<(), Object> {
    let obj = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let value = crate::evaluator::methods::get_property(&obj, name, pos);
    if value.is_runtime_error() {
        return Err(value);
    }
    stack.push(value);
    Ok(())
}

pub(super) fn get_index_stack(stack: &mut Vec<Object>, pos: Position) -> Result<(), Object> {
    let key = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let obj = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let value = crate::evaluator::methods::get_index(&obj, &key, pos);
    if value.is_runtime_error() {
        return Err(value);
    }
    stack.push(value);
    Ok(())
}

pub(super) fn set_index_stack(stack: &mut Vec<Object>, pos: Position) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let key = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let obj = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let assigned = assign_index(&obj, &key, value, pos)?;
    stack.push(assigned);
    Ok(())
}

pub(super) fn iter_keys_stack(stack: &mut Vec<Object>, pos: Position) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let elements = crate::evaluator::eval_core::iterable_keys(&value)
        .into_iter()
        .map(str_obj)
        .collect();
    stack.push(Object::Array(Rc::new(RefCell::new(ArrayData { elements }))));
    Ok(())
}

pub(super) fn iter_values_stack(
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

pub(super) fn iter_next_stack(
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

pub(super) fn len_stack(stack: &mut Vec<Object>, pos: Position) -> Result<(), Object> {
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

fn check_type_annotation(
    env: &EnvRef,
    name: &str,
    value: &Object,
    type_anno: &TypeAnnotation,
    pos: Position,
) -> Result<(), Object> {
    if !env.borrow().vm.type_check.load(Ordering::Relaxed)
        || value_matches_type_annotation(value, type_anno)
    {
        return Ok(());
    }
    Err(new_error(
        pos,
        format!(
            "TypeError: cannot assign {} to '{}: {}'",
            value.type_tag(),
            name,
            type_anno
        ),
    ))
}

fn binding_info(env: &EnvRef, name: &str) -> Option<(bool, Option<TypeAnnotation>)> {
    let mut scope = Some(env.clone());
    while let Some(env) = scope {
        let borrowed = env.borrow();
        if let Some(binding) = borrowed.bindings.get(name) {
            return Some((binding.is_const, binding.type_anno.clone()));
        }
        scope = borrowed.parent.clone();
    }
    None
}

pub(super) fn read_string_const(
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

pub(super) fn push_packed_arg(
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

pub(super) fn spread_stack(stack: &mut Vec<Object>, pos: Position) -> Result<(), Object> {
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

pub(super) fn call_stack(
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

pub(super) fn call_spread_stack(
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

pub(super) fn construct_stack(
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

pub(super) fn call_value(
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

pub(super) fn construct_value(
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

pub(super) fn build_class_to_stack(
    stack: &mut Vec<Object>,
    chunk: &Chunk,
    env: &EnvRef,
    class_idx: usize,
    pos: Position,
) -> Result<(), Object> {
    let Some(class_decl) = chunk.classes.get(class_idx) else {
        return Err(new_error(
            pos,
            format!("VMError: missing class declaration {}", class_idx),
        ));
    };
    let class = crate::bytecode::class::build_class(
        class_decl,
        env,
        &crate::bytecode::resolve::ResolutionMap::default(),
    )?;
    stack.push(class);
    Ok(())
}

pub(super) fn closure_from_proto(
    proto: Rc<FunctionProto>,
    upvalues: Vec<Rc<Upvalue>>,
    home_env: EnvRef,
) -> Object {
    Object::Closure(Rc::new(super::closure::ClosureData {
        upvalue_names: proto
            .upvalue_desc
            .iter()
            .map(|desc| desc.name.clone())
            .collect(),
        proto,
        upvalues,
        home_env,
    }))
}

pub(super) fn read_function_proto_operand(
    chunk: &Chunk,
    ip: &mut usize,
    opcode: &'static str,
) -> Result<(Rc<FunctionProto>, Position), Object> {
    let (proto_idx, pos) = read_usize_operand_with_pos(chunk, ip, opcode)?;
    let Some(proto) = chunk.protos.get(proto_idx).cloned() else {
        return Err(new_error(
            pos,
            format!("VMError: missing function prototype {}", proto_idx),
        ));
    };
    Ok((proto, pos))
}

pub(super) fn assign_property(
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

pub(super) fn assign_index(
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

pub(crate) fn value_matches_type_annotation(value: &Object, anno: &TypeAnnotation) -> bool {
    if anno.optional && matches!(value, Object::Null | Object::Undefined) {
        return true;
    }
    match anno.kind {
        TypeKind::Union => anno
            .union
            .iter()
            .any(|member| value_matches_type_annotation(value, member)),
        TypeKind::Array => match value {
            Object::Array(items) => {
                let Some(inner) = &anno.array_of else {
                    return true;
                };
                items
                    .borrow()
                    .elements
                    .iter()
                    .all(|item| value_matches_type_annotation(item, inner))
            }
            _ => false,
        },
        TypeKind::Object => is_object_like(value),
        TypeKind::Function => is_function_like(value),
        TypeKind::Primitive => match anno.name.as_str() {
            "any" | "unknown" => true,
            "number" => matches!(value, Object::Number(_)),
            "string" => matches!(value, Object::String(_)),
            "boolean" | "bool" => matches!(value, Object::Boolean(_)),
            "null" => matches!(value, Object::Null),
            "undefined" | "void" => matches!(value, Object::Undefined),
            "object" => is_object_like(value),
            "function" => is_function_like(value),
            _ => true,
        },
    }
}

fn is_object_like(value: &Object) -> bool {
    matches!(
        value,
        Object::Hash(_)
            | Object::Array(_)
            | Object::Instance(_)
            | Object::Map(_)
            | Object::Set(_)
            | Object::Date(_)
            | Object::Regexp(_)
            | Object::Error(_)
            | Object::Null
    )
}

fn is_function_like(value: &Object) -> bool {
    matches!(
        value,
        Object::Function(_) | Object::Builtin(_) | Object::Class(_) | Object::Closure(_)
    )
}

pub(super) fn await_value(value: Object, env: &EnvRef, pos: Position) -> Result<Object, Object> {
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
