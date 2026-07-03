//! Lexical environments (scopes) with a parent chain for closures.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::TypeAnnotation;

use super::value::{EnvRef, Object};
use super::vm::VirtualMachine;

/// A single binding.
#[derive(Clone)]
pub struct Binding {
    pub value: Object,
    pub is_const: bool,
    pub type_anno: Option<TypeAnnotation>,
}

/// A lexical scope.
pub struct Environment {
    pub bindings: HashMap<String, Binding>,
    pub parent: Option<EnvRef>,
    pub vm: Rc<VirtualMachine>,
    /// `this` binding for the current method/constructor call.
    pub this: Option<Object>,
    /// The class whose constructor is currently executing (for `super`).
    pub constructor_class: Option<Rc<RefCell<super::Class>>>,
    pub super_called: bool,
    pub module_dir: String,
}

impl Environment {
    /// Create a fresh root environment bound to a VM.
    pub fn new_root(vm: Rc<VirtualMachine>) -> EnvRef {
        Rc::new(RefCell::new(Environment {
            bindings: HashMap::new(),
            parent: None,
            vm,
            this: None,
            constructor_class: None,
            super_called: false,
            module_dir: String::new(),
        }))
    }

    /// Create a child scope of `parent`.
    pub fn child(parent: &EnvRef) -> EnvRef {
        let p = parent.borrow_mut();
        Rc::new(RefCell::new(Environment {
            bindings: HashMap::new(),
            parent: Some(parent.clone()),
            vm: p.vm.clone(),
            this: p.this.clone(),
            constructor_class: p.constructor_class.clone(),
            super_called: p.super_called,
            module_dir: p.module_dir.clone(),
        }))
    }

    /// Look up a name in this scope and its parents.
    pub fn get(&self, name: &str) -> Option<Object> {
        if let Some(b) = self.bindings.get(name) {
            return Some(b.value.clone());
        }
        if let Some(p) = &self.parent {
            return p.borrow_mut().get(name);
        }
        self.vm.get_global(name)
    }

    pub fn has(&self, name: &str) -> bool {
        if self.bindings.contains_key(name) {
            return true;
        }
        if let Some(p) = &self.parent {
            return p.borrow_mut().has(name);
        }
        self.vm.has_global(name)
    }

    /// Set a binding in this scope (always creates/overwrites here).
    pub fn set_here(&mut self, name: impl Into<String>, value: Object) {
        self.bindings.insert(
            name.into(),
            Binding {
                value,
                is_const: false,
                type_anno: None,
            },
        );
    }

    pub fn set_const_here(&mut self, name: impl Into<String>, value: Object) {
        self.bindings.insert(
            name.into(),
            Binding {
                value,
                is_const: true,
                type_anno: None,
            },
        );
    }

    pub fn set_typed(
        &mut self,
        name: impl Into<String>,
        value: Object,
        anno: Option<TypeAnnotation>,
    ) {
        self.bindings.insert(
            name.into(),
            Binding {
                value,
                is_const: false,
                type_anno: anno,
            },
        );
    }

    pub fn set_typed_const(
        &mut self,
        name: impl Into<String>,
        value: Object,
        anno: Option<TypeAnnotation>,
    ) {
        self.bindings.insert(
            name.into(),
            Binding {
                value,
                is_const: true,
                type_anno: anno,
            },
        );
    }

    /// Assign to an existing binding somewhere in the chain. Returns
    /// `(found, is_const)`.
    pub fn assign(&mut self, name: &str, value: Object) -> (bool, bool) {
        if let Some(b) = self.bindings.get_mut(name) {
            if b.is_const {
                return (true, true);
            }
            b.value = value;
            return (true, false);
        }
        if let Some(p) = &self.parent {
            let r = p.borrow_mut().assign(name, value);
            if r.0 {
                return r;
            }
        }
        if self.vm.has_global(name) {
            return (true, true);
        }
        (false, false)
    }
}
