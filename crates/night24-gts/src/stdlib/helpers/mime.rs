use super::*;

// ---------------------------------------------------------------------------
// mime: a built-in extension<->type table plus format/parse helpers.
// ---------------------------------------------------------------------------

pub(crate) fn value_to_string(obj: &Object) -> String {
    match obj {
        Object::String(s) => s.to_string(),
        Object::Number(n) => format_number(*n),
        Object::Boolean(b) => b.to_string(),
        Object::Null => "null".to_string(),
        Object::Hash(_) => {
            // Simple JSON serialization
            format!("{:?}", obj)
        }
        _ => format!("{:?}", obj),
    }
}
