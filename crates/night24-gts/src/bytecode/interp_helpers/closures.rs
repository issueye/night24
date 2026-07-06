use crate::ast::Position;
use crate::object::{new_error, EnvRef, Object};
use std::collections::BTreeMap;
use std::rc::Rc;

use super::super::chunk::Chunk;
use super::super::closure::{FunctionProto, UpvalueSource};
use super::super::upvalue::Upvalue;
use super::read_usize_operand_with_pos;

pub(in crate::bytecode) fn close_open_upvalues_from(
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

fn capture_proto_upvalues(
    proto: &Rc<FunctionProto>,
    env: &EnvRef,
    open_upvalues: &mut BTreeMap<usize, Vec<Rc<Upvalue>>>,
    current_upvalues: &[Rc<Upvalue>],
) -> Result<Vec<Rc<Upvalue>>, Object> {
    let mut captured = Vec::with_capacity(proto.upvalue_desc.len());
    for desc in &proto.upvalue_desc {
        match desc.source {
            UpvalueSource::LocalSlot(slot) => {
                if let Some(value) = env.borrow().get(&desc.name) {
                    captured.push(Upvalue::new_closed(value));
                } else {
                    captured.push(capture_open_upvalue(open_upvalues, slot as usize));
                }
            }
            UpvalueSource::ParentUpvalue(index) => {
                let Some(parent) = current_upvalues.get(index as usize) else {
                    return Err(new_error(
                        proto.pos.clone(),
                        format!(
                            "VMError: missing parent upvalue {} for closure {}",
                            index, proto.name
                        ),
                    ));
                };
                captured.push(parent.clone());
            }
        }
    }
    Ok(captured)
}

pub(in crate::bytecode) fn capture_open_upvalue(
    open_upvalues: &mut BTreeMap<usize, Vec<Rc<Upvalue>>>,
    slot: usize,
) -> Rc<Upvalue> {
    let entry = open_upvalues.entry(slot).or_default();
    if let Some(existing) = entry.iter().find(|upvalue| upvalue.is_open()) {
        return existing.clone();
    }
    let upvalue = Upvalue::new_open(slot);
    entry.push(upvalue.clone());
    upvalue
}

pub(in crate::bytecode) fn build_class_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    env: &EnvRef,
) -> Result<(), Object> {
    let (class_idx, pos) = read_usize_operand_with_pos(chunk, ip, "NEW_CLASS")?;
    build_class_to_stack(stack, chunk, env, class_idx, pos)
}

fn build_class_to_stack(
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

fn closure_from_proto(
    proto: Rc<FunctionProto>,
    upvalues: Vec<Rc<Upvalue>>,
    home_env: EnvRef,
) -> Object {
    Object::Closure(Rc::new(super::super::closure::ClosureData {
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

fn read_function_proto_operand(
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

pub(in crate::bytecode) fn push_closure_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    env: &EnvRef,
    open_upvalues: &mut BTreeMap<usize, Vec<Rc<Upvalue>>>,
    current_upvalues: &[Rc<Upvalue>],
) -> Result<(), Object> {
    let (proto, _pos) = read_function_proto_operand(chunk, ip, "CLOSURE")?;
    let upvalues = capture_proto_upvalues(&proto, env, open_upvalues, current_upvalues)?;
    stack.push(closure_from_proto(proto, upvalues, env.clone()));
    Ok(())
}
