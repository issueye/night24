//! Call-frame model for bytecode function execution.
//!
//! The interpreter still executes function calls by recursively entering a
//! child chunk in stage 3. This module records the frame shape that later
//! stages will move into an explicit frame stack.

use std::cell::RefCell;
use std::rc::Rc;

use crate::object::{EnvRef, Object};

use super::closure::{FunctionProto, UpvalueDesc};
use super::upvalue::Upvalue;

/// A captured value visible to a call frame.
#[derive(Clone)]
pub enum FrameUpvalue {
    /// Stage-4 placeholder: the descriptor is known, but values still resolve
    /// through the environment chain in the current VM.
    Deferred(UpvalueDesc),
    /// A closed-over value after an outer frame exits.
    Closed(Rc<RefCell<Object>>),
    /// A runtime upvalue captured from an outer bytecode frame.
    Captured(Rc<Upvalue>),
}

/// Runtime call-frame state.
pub struct CallFrame {
    /// Instruction pointer within `proto.chunk`.
    pub ip: usize,
    pub proto: Rc<FunctionProto>,
    /// Local slot storage. Stage 3 mirrors bound parameters here for metadata;
    /// reads/writes still use dynamic names until the stage-4 resolver lands.
    pub slots: Vec<Object>,
    pub upvalues: Vec<FrameUpvalue>,
    pub this: Option<Object>,
    /// Base index in the VM value stack for this frame.
    pub slot_base: usize,
}

impl CallFrame {
    pub fn new(
        proto: Rc<FunctionProto>,
        slots: Vec<Object>,
        upvalues: Vec<FrameUpvalue>,
        this: Option<Object>,
        slot_base: usize,
    ) -> Self {
        CallFrame {
            ip: 0,
            proto,
            slots,
            upvalues,
            this,
            slot_base,
        }
    }

    /// Build the stage-3 frame metadata from a bound call environment.
    pub fn from_bound_env(proto: Rc<FunctionProto>, env: &EnvRef, slot_base: usize) -> Self {
        let borrowed = env.borrow();
        let slots = proto
            .param_slots
            .iter()
            .map(|p| {
                borrowed
                    .bindings
                    .get(&p.name)
                    .map(|b| b.value.clone())
                    .unwrap_or(Object::Undefined)
            })
            .collect();
        let this = borrowed.this.clone();
        drop(borrowed);

        let upvalues = proto
            .upvalue_desc
            .iter()
            .cloned()
            .map(FrameUpvalue::Deferred)
            .collect();

        CallFrame::new(proto, slots, upvalues, this, slot_base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BlockStmt, Param, Position};
    use crate::object::{Environment, VirtualMachine};

    #[test]
    fn frame_mirrors_bound_params_into_slots() {
        let proto = FunctionProto::new(
            "f",
            vec![Param {
                pos: Position::default(),
                name: "value".into(),
                type_anno: None,
                default: None,
                spread: false,
                optional: false,
            }],
            BlockStmt::default(),
            false,
            false,
            None,
            Position::default(),
        );
        let env = Environment::new_root(VirtualMachine::new());
        env.borrow_mut()
            .set_here("value", Object::String(Rc::new("ok".into())));

        let frame = CallFrame::from_bound_env(proto, &env, 7);

        assert_eq!(frame.ip, 0);
        assert_eq!(frame.slot_base, 7);
        assert!(matches!(&frame.slots[0], Object::String(s) if &**s == "ok"));
    }
}
