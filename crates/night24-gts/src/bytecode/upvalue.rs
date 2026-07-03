//! Upvalue storage for bytecode closures.
//!
//! Stage 4 starts with the data model only. An open upvalue points at a slot
//! in an active call frame; closing it copies the current slot value into
//! heap-backed storage so returned closures can keep using it.

use std::cell::RefCell;
use std::rc::Rc;

use crate::object::Object;

#[derive(Clone)]
pub struct Upvalue {
    state: RefCell<UpvalueState>,
}

#[derive(Clone)]
pub enum UpvalueState {
    Open { slot: usize },
    Closed(Rc<RefCell<Object>>),
}

impl Upvalue {
    pub fn new_open(slot: usize) -> Rc<Self> {
        Rc::new(Upvalue {
            state: RefCell::new(UpvalueState::Open { slot }),
        })
    }

    pub fn new_closed(value: Object) -> Rc<Self> {
        Rc::new(Upvalue {
            state: RefCell::new(UpvalueState::Closed(Rc::new(RefCell::new(value)))),
        })
    }

    pub fn is_open(&self) -> bool {
        matches!(*self.state.borrow(), UpvalueState::Open { .. })
    }

    pub fn open_slot(&self) -> Option<usize> {
        match *self.state.borrow() {
            UpvalueState::Open { slot } => Some(slot),
            UpvalueState::Closed(_) => None,
        }
    }

    pub fn get(&self, slots: &[Object]) -> Option<Object> {
        match &*self.state.borrow() {
            UpvalueState::Open { slot } => slots.get(*slot).cloned(),
            UpvalueState::Closed(value) => Some(value.borrow().clone()),
        }
    }

    pub fn set(&self, slots: &mut [Object], value: Object) -> bool {
        match &*self.state.borrow() {
            UpvalueState::Open { slot } => {
                let Some(target) = slots.get_mut(*slot) else {
                    return false;
                };
                *target = value;
                true
            }
            UpvalueState::Closed(cell) => {
                *cell.borrow_mut() = value;
                true
            }
        }
    }

    pub fn close_from_slots(&self, slots: &[Object]) -> bool {
        let value = match &*self.state.borrow() {
            UpvalueState::Open { slot } => match slots.get(*slot) {
                Some(value) => value.clone(),
                None => return false,
            },
            UpvalueState::Closed(_) => return true,
        };
        *self.state.borrow_mut() = UpvalueState::Closed(Rc::new(RefCell::new(value)));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_upvalue_reads_and_writes_stack_slot() {
        let upvalue = Upvalue::new_open(1);
        let mut slots = vec![Object::Number(1.0), Object::Number(2.0)];

        assert!(upvalue.is_open());
        assert_eq!(upvalue.open_slot(), Some(1));
        assert!(matches!(upvalue.get(&slots), Some(Object::Number(2.0))));

        assert!(upvalue.set(&mut slots, Object::Number(7.0)));
        assert!(matches!(slots[1], Object::Number(7.0)));
    }

    #[test]
    fn closing_detaches_value_from_stack_slot() {
        let upvalue = Upvalue::new_open(0);
        let mut slots = vec![Object::Number(3.0)];

        assert!(upvalue.close_from_slots(&slots));
        assert!(!upvalue.is_open());

        slots[0] = Object::Number(9.0);
        assert!(matches!(upvalue.get(&slots), Some(Object::Number(3.0))));

        assert!(upvalue.set(&mut slots, Object::Number(4.0)));
        assert!(matches!(slots[0], Object::Number(9.0)));
        assert!(matches!(upvalue.get(&slots), Some(Object::Number(4.0))));
    }
}
