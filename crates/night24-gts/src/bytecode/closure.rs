//! Function prototype and closure data for the bytecode VM.
//!
//! A `FunctionProto` is the compiled representation of a function body. To
//! avoid a `Chunk`-inside-`Object` cycle (and the `PartialEq` headaches that
//! come with it), the proto holds an AST body reference plus a lazily-compiled
//! chunk. Compilation happens on first call and is cached.
//!
//! Stage 3 scope: parameter binding and a fresh environment per call, with the
//! body executed via the bytecode interpreter. Upvalue capture (closures over
//! local variables) is stage 4; stage-3 closures work only because the captured
//! name resolves through the global name table (e.g. recursive `fact`).

use std::cell::RefCell;
use std::rc::Rc;

use super::upvalue::Upvalue;
use crate::ast::{BlockStmt, Param, Position, TypeAnnotation};

/// Parameter metadata used by the bytecode call layer.
///
/// Stage 3 still binds parameters through the shared tree-walker helper so
/// defaults/rest semantics stay identical. These slots record the layout that
/// the stage-4 local-slot compiler will target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSlot {
    pub name: String,
    pub slot: u16,
    pub has_default: bool,
    pub is_rest: bool,
    pub optional: bool,
}

/// Where an upvalue should be captured from when `OpClosure` runs.
///
/// The current stage records an empty list because name lookup still goes
/// through environments. Stage 4 will populate this during lexical resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpvalueDesc {
    pub name: String,
    pub source: UpvalueSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpvalueSource {
    LocalSlot(u16),
    ParentUpvalue(u16),
}

/// Compiled-ish function body. The chunk is filled in lazily on first call so
/// the proto can live inside an `Object` without an eager compile cycle.
pub struct FunctionProto {
    pub name: String,
    pub params: Vec<Param>,
    /// Number of positional parameters before the first default/rest/optional
    /// marker. Mirrors JavaScript-like `Function.length` arity semantics.
    pub arity: usize,
    /// Parameter-to-slot layout for the future local-slot VM path.
    pub param_slots: Vec<ParamSlot>,
    /// Upvalue capture descriptors. Empty until stage 4 lexical resolution.
    pub upvalue_desc: Vec<UpvalueDesc>,
    pub body: Rc<BlockStmt>,
    pub is_async: bool,
    pub lexical_this: bool,
    pub return_t: Option<TypeAnnotation>,
    pub pos: Position,
    /// Lazily-compiled bytecode for the body. `None` until first call.
    pub chunk: RefCell<Option<Rc<super::Chunk>>>,
}

/// A closure value: a function proto bound to the environment captured at
/// definition time (the defining scope, used for name resolution).
pub struct ClosureData {
    pub proto: Rc<FunctionProto>,
    /// Runtime upvalues captured when `OpClosure` executes. Stage 4.3 wires
    /// capture/close lifetime; 4.4 will make load/store opcodes consume these.
    pub upvalues: Vec<Rc<Upvalue>>,
    /// Name order matching `upvalues`. This is a stage-4 bridge for the
    /// remaining name-table execution path until local-slot reads replace it.
    pub upvalue_names: Vec<String>,
    /// The environment captured at definition. Stage 3 uses it as the parent
    /// for the call scope so globals resolve; stage 4 adds true upvalue
    /// capture for locals.
    pub home_env: crate::object::EnvRef,
}

impl FunctionProto {
    pub fn new(
        name: impl Into<String>,
        params: Vec<Param>,
        body: BlockStmt,
        is_async: bool,
        lexical_this: bool,
        return_t: Option<TypeAnnotation>,
        pos: Position,
    ) -> Rc<FunctionProto> {
        FunctionProto::with_upvalues(
            name,
            params,
            body,
            is_async,
            lexical_this,
            return_t,
            pos,
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_upvalues(
        name: impl Into<String>,
        params: Vec<Param>,
        body: BlockStmt,
        is_async: bool,
        lexical_this: bool,
        return_t: Option<TypeAnnotation>,
        pos: Position,
        upvalue_desc: Vec<UpvalueDesc>,
    ) -> Rc<FunctionProto> {
        let arity = required_arity(&params);
        let param_slots = build_param_slots(&params);
        Rc::new(FunctionProto {
            name: name.into(),
            params,
            arity,
            param_slots,
            upvalue_desc,
            body: Rc::new(body),
            is_async,
            lexical_this,
            return_t,
            pos,
            chunk: RefCell::new(None),
        })
    }
}

fn required_arity(params: &[Param]) -> usize {
    params
        .iter()
        .take_while(|p| !p.spread && !p.optional && p.default.is_none())
        .count()
}

fn build_param_slots(params: &[Param]) -> Vec<ParamSlot> {
    params
        .iter()
        .enumerate()
        .map(|(slot, p)| ParamSlot {
            name: p.name.clone(),
            slot: slot as u16,
            has_default: p.default.is_some(),
            is_rest: p.spread,
            optional: p.optional,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Expr;

    fn param(name: &str, default: Option<Expr>, spread: bool, optional: bool) -> Param {
        Param {
            pos: Position::default(),
            name: name.into(),
            type_anno: None,
            default,
            spread,
            optional,
        }
    }

    #[test]
    fn function_proto_records_arity_and_param_slots() {
        let proto = FunctionProto::new(
            "f",
            vec![
                param("a", None, false, false),
                param(
                    "b",
                    Some(Expr::Undefined(crate::ast::UndefinedLit {
                        pos: Position::default(),
                    })),
                    false,
                    false,
                ),
                param("rest", None, true, false),
            ],
            BlockStmt::default(),
            false,
            false,
            None,
            Position::default(),
        );

        assert_eq!(proto.arity, 1);
        assert_eq!(proto.param_slots.len(), 3);
        assert_eq!(proto.param_slots[0].name, "a");
        assert_eq!(proto.param_slots[0].slot, 0);
        assert!(proto.param_slots[1].has_default);
        assert!(proto.param_slots[2].is_rest);
        assert!(proto.upvalue_desc.is_empty());
    }
}
