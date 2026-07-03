use super::*;

pub(crate) fn deep_equal(a: &Object, b: &Object) -> bool {
    match (a, b) {
        (Object::Number(x), Object::Number(y)) => x == y,
        (Object::String(x), Object::String(y)) => x == y,
        (Object::Boolean(x), Object::Boolean(y)) => x == y,
        (Object::Null, Object::Null) | (Object::Undefined, Object::Undefined) => true,
        (Object::Array(x), Object::Array(y)) => {
            let xb = x.borrow();
            let yb = y.borrow();
            xb.elements.len() == yb.elements.len()
                && xb
                    .elements
                    .iter()
                    .zip(yb.elements.iter())
                    .all(|(p, q)| deep_equal(p, q))
        }
        (Object::Hash(x), Object::Hash(y)) => {
            let xb = x.borrow();
            let yb = y.borrow();
            xb.entries.len() == yb.entries.len()
                && xb.entries.iter().all(|(k, v)| {
                    yb.entries
                        .iter()
                        .any(|(yk, yv)| yk == k && deep_equal(v, yv))
                })
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// test: a script-side test runner with describe/it/expect.
//
// State is held in thread-local storage so the runner survives nested
// describe() calls and is collected on run().
// ---------------------------------------------------------------------------

pub(crate) fn is_truthy(value: &Object) -> bool {
    match value {
        Object::Boolean(b) => *b,
        Object::Number(n) => *n != 0.0,
        Object::String(s) => !s.is_empty(),
        Object::Null | Object::Undefined => false,
        _ => true,
    }
}

/// Invoke a script Function/Builtin with arguments, returning its result.
pub(crate) fn call_script_function(func: &Object, env: &EnvRef, args: &[Object]) -> Object {
    crate::evaluator::expressions::apply_function(func, env, args, None, Position::default())
}

// ---------------------------------------------------------------------------
// archive/zip: stateless list/extract/create.
// ---------------------------------------------------------------------------
