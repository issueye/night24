use std::rc::Rc;

use crate::ast::{ClassDecl, Expr, Position, Property};
use crate::object::{format_number, new_error, Object};

use super::chunk::Chunk;

pub(super) fn add_class_decl(chunk: &mut Chunk, decl: ClassDecl) -> u16 {
    let idx = chunk.classes.len() as u16;
    chunk.classes.push(Rc::new(decl));
    idx
}

pub(super) fn object_property_key(prop: &Property) -> Result<String, Object> {
    if prop.shorthand {
        if let Expr::Ident(i) = &prop.key {
            return Ok(i.name.clone());
        }
    }
    let key = object_property_key_expr(&prop.key);
    if key.is_empty() {
        Err(unsupported(prop.pos.clone(), "object property key"))
    } else {
        Ok(key)
    }
}

pub(super) fn object_property_key_expr(expr: &Expr) -> String {
    match expr {
        Expr::Ident(i) => i.name.clone(),
        Expr::String(s) => crate::evaluator::eval_core::strip_quotes(&s.literal),
        Expr::Number(n) => format_number(n.value),
        _ => String::new(),
    }
}

fn unsupported(pos: Position, what: &str) -> Object {
    new_error(
        pos,
        format!("CompileError: bytecode VM does not yet support {}", what),
    )
}
