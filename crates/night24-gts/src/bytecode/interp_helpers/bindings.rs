use crate::ast::{Position, TypeAnnotation, TypeKind};
use crate::object::{new_error, EnvRef, Object};
use std::rc::Rc;
use std::sync::atomic::Ordering;

use super::super::chunk::Chunk;
use super::super::upvalue::Upvalue;
use super::{
    read_byte_operand_with_pos, read_name_operand, read_string_operand, read_type_operand,
    stack_underflow,
};

pub(in crate::bytecode) fn load_this(stack: &mut Vec<Object>, env: &EnvRef) {
    let value = env.borrow().this.clone().unwrap_or(Object::Undefined);
    stack.push(value);
}

fn load_local(stack: &mut Vec<Object>, slot: usize, pos: Position) -> Result<(), Object> {
    let Some(value) = stack.get(slot).cloned() else {
        return Err(new_error(
            pos,
            format!("VMError: LOAD_LOCAL slot {} out of range", slot),
        ));
    };
    stack.push(value);
    Ok(())
}

pub(in crate::bytecode) fn load_local_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
) -> Result<(), Object> {
    let (slot, pos) = read_byte_operand_with_pos(chunk, ip, "LOAD_LOCAL")?;
    load_local(stack, slot, pos)
}

fn store_local(stack: &mut Vec<Object>, slot: usize, pos: Position) -> Result<(), Object> {
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

pub(in crate::bytecode) fn store_local_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
) -> Result<(), Object> {
    let (slot, pos) = read_byte_operand_with_pos(chunk, ip, "STORE_LOCAL")?;
    store_local(stack, slot, pos)
}

fn load_upvalue(
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

pub(in crate::bytecode) fn load_upvalue_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    upvalues: &[Rc<Upvalue>],
) -> Result<(), Object> {
    let (index, pos) = read_byte_operand_with_pos(chunk, ip, "LOAD_UPVALUE")?;
    load_upvalue(stack, upvalues, index, pos)
}

fn store_upvalue(
    stack: &mut [Object],
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

pub(in crate::bytecode) fn store_upvalue_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    upvalues: &[Rc<Upvalue>],
) -> Result<(), Object> {
    let (index, pos) = read_byte_operand_with_pos(chunk, ip, "STORE_UPVALUE")?;
    store_upvalue(stack, upvalues, index, pos)
}

fn load_name(
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

pub(in crate::bytecode) fn load_name_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    env: &EnvRef,
) -> Result<(), Object> {
    let (name, pos) = read_string_operand(chunk, ip, "LOAD_NAME")?;
    load_name(stack, env, &name, pos)
}

fn store_name(
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

pub(in crate::bytecode) fn store_name_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    env: &EnvRef,
) -> Result<(), Object> {
    let (name, is_const, pos) = read_name_operand(chunk, ip, "STORE_NAME")?;
    store_name(stack, env, name, is_const, pos)
}

fn store_typed_name(
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

pub(in crate::bytecode) fn store_typed_name_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    env: &EnvRef,
) -> Result<(), Object> {
    let (name, is_const, pos) = read_name_operand(chunk, ip, "STORE_TYPED_NAME")?;
    let type_anno = read_type_operand(chunk, ip, "STORE_TYPED_NAME", pos.clone())?;
    store_typed_name(stack, env, name, is_const, type_anno, pos)
}

fn assign_name(
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

pub(in crate::bytecode) fn assign_name_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    env: &EnvRef,
) -> Result<(), Object> {
    let (name, pos) = read_string_operand(chunk, ip, "ASSIGN_NAME")?;
    assign_name(stack, env, &name, pos)
}

fn load_global(
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

pub(in crate::bytecode) fn load_global_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    env: &EnvRef,
) -> Result<(), Object> {
    let (name, pos) = read_string_operand(chunk, ip, "LOAD_GLOBAL")?;
    load_global(stack, env, &name, pos)
}

fn store_global(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    name: String,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos))?;
    env.borrow().vm.set_global(name, value);
    Ok(())
}

pub(in crate::bytecode) fn store_global_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    env: &EnvRef,
) -> Result<(), Object> {
    let (name, is_const, pos) = read_name_operand(chunk, ip, "STORE_GLOBAL")?;
    // The high bit is accepted for parity with `StoreName`, but globals do not
    // track const-ness here; declarations still go through `StoreName`.
    let _ = is_const;
    store_global(stack, env, name, pos)
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
