//! Bytecode-side class construction.
//!
//! Stage 5.3 keeps the shared object model (`Object::Class` /
//! `Object::Instance`) but builds user classes from bytecode closures so
//! methods run through the VM instead of falling back to tree-walker functions.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{ClassDecl, ClassMemberKind};
use crate::object::{new_error, Class, EnvRef, Object};

/// Build a class value from an AST declaration using bytecode closures for
/// methods and constructors.
pub fn build_class(
    decl: &ClassDecl,
    env: &EnvRef,
    resolutions: &super::resolve::ResolutionMap,
) -> Result<Object, Object> {
    let mut class = Class {
        name: decl.name.clone(),
        super_: None,
        methods: HashMap::new(),
        fields: HashMap::new(),
        field_types: HashMap::new(),
        statics: HashMap::new(),
        static_types: HashMap::new(),
        native_ctor: None,
        pos: decl.pos.clone(),
    };

    if let Some(super_expr) = &decl.super_ {
        let sv = crate::evaluator::expressions::eval_expr(super_expr, env);
        match &sv {
            Object::Class(sc) => {
                class.super_ = Some(sc.clone());
                let scb = sc.borrow();
                for (k, v) in scb.methods.iter() {
                    if k != "constructor" {
                        class.methods.insert(k.clone(), v.clone());
                    }
                }
                for (k, v) in scb.fields.iter() {
                    class.fields.insert(k.clone(), v.clone());
                }
            }
            Object::Builtin(b) if crate::evaluator::expressions::is_error_class_name(&b.name) => {
                class.super_ = Some(crate::evaluator::expressions::native_error_class(
                    env,
                    &b.name,
                    decl.pos.clone(),
                )?);
            }
            _ => {
                return Err(new_error(
                    decl.pos.clone(),
                    "TypeError: superclass must be a class",
                ))
            }
        }
    }

    for member in &decl.body.members {
        match member.kind {
            ClassMemberKind::Method | ClassMemberKind::Constructor => {
                let Some(body) = &member.body else {
                    continue;
                };
                let proto = super::compiler::compile_method_proto(
                    &member.name,
                    member.params.clone(),
                    body.clone(),
                    member.is_async,
                    member.type_anno.clone(),
                    member.pos.clone(),
                    resolutions,
                )?;
                let closure = Object::Closure(Rc::new(super::closure::ClosureData {
                    upvalue_names: proto
                        .upvalue_desc
                        .iter()
                        .map(|desc| desc.name.clone())
                        .collect(),
                    proto,
                    upvalues: Vec::new(),
                    home_env: env.clone(),
                }));
                if member.is_static {
                    class.statics.insert(member.name.clone(), closure);
                } else {
                    class.methods.insert(member.name.clone(), closure);
                }
            }
            ClassMemberKind::Field => {
                let val = match &member.default_val {
                    Some(e) => {
                        let v = crate::evaluator::expressions::eval_expr(e, env);
                        if v.is_runtime_error() {
                            return Err(v);
                        }
                        v
                    }
                    None => Object::Undefined,
                };
                if member.is_static {
                    class.statics.insert(member.name.clone(), val);
                    if let Some(t) = &member.type_anno {
                        class.static_types.insert(member.name.clone(), t.clone());
                    }
                } else {
                    class.fields.insert(member.name.clone(), val);
                    if let Some(t) = &member.type_anno {
                        class.field_types.insert(member.name.clone(), t.clone());
                    }
                }
            }
        }
    }

    Ok(Object::Class(Rc::new(RefCell::new(class))))
}
