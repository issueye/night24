use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, str_obj, CallContext, HashData, Object};

pub(crate) fn schema_module() -> Object {
    module(vec![
        ("validate", native("schema.validate", schema_validate)),
        ("assert", native("schema.assert", schema_assert)),
    ])
}

pub(crate) fn schema_validate(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.len() < 2 {
        return new_error(ctx.pos.clone(), "schema.validate requires schema and value");
    }
    let (schema, value) = (&args[0], &args[1]);
    let errors = match validate_schema(schema, value, "$") {
        Ok(errs) => errs,
        Err(e) => return new_error(ctx.pos.clone(), e),
    };
    let valid = errors.is_empty();
    let hash = Rc::new(RefCell::new(HashData::default()));
    hash.borrow_mut().set("valid", bool_obj(valid));
    hash.borrow_mut()
        .set("errors", array(errors.into_iter().map(str_obj).collect()));
    Object::Hash(hash)
}

pub(crate) fn schema_assert(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.len() < 2 {
        return new_error(ctx.pos.clone(), "schema.assert requires schema and value");
    }
    let (schema, value) = (&args[0], &args[1]);
    match validate_schema(schema, value, "$") {
        Ok(errs) => {
            if let Some(first) = errs.first() {
                new_error(ctx.pos.clone(), format!("schema.assert: {}", first))
            } else {
                value.clone()
            }
        }
        Err(e) => new_error(ctx.pos.clone(), e),
    }
}

/// Validate `value` against `schema` rooted at `path`. Returns the list of
/// error messages (empty on success) or an Error-prefixed string on misuse.
fn validate_schema(schema: &Object, value: &Object, path: &str) -> Result<Vec<String>, String> {
    let schema_hash = match schema {
        Object::Hash(h) => h.clone(),
        _ => return Err("schema.validate: schema must be an object".to_string()),
    };
    let mut errors = Vec::new();
    let s = schema_hash.borrow();

    if let Some(Object::String(t)) = s.get("type") {
        let type_text = t.as_str();
        if !type_matches(value, type_text) {
            errors.push(format!(
                "{} expected {}, got {}",
                path,
                type_text,
                value_type_name(value)
            ));
        }
    }
    if let Some(Object::Array(enum_arr)) = s.get("enum") {
        let matched = enum_arr
            .borrow()
            .elements
            .iter()
            .any(|e| deep_equal(value, e));
        if !matched {
            errors.push(format!(
                "{} must be one of {}",
                path,
                enum_arr
                    .borrow()
                    .elements
                    .iter()
                    .map(|e| e.inspect())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if let Object::Hash(_) = value {
        if let Some(Object::Array(required)) = s.get("required") {
            let value_hash = value_as_hash(value);
            for req in &required.borrow().elements {
                if let Object::String(key) = req {
                    let present = value_hash
                        .as_ref()
                        .map(|h| h.borrow().contains(key.as_str()))
                        .unwrap_or(false);
                    if !present {
                        errors.push(format!("{}.{} is required", path, key));
                    }
                }
            }
        }
        if let Some(Object::Hash(properties)) = s.get("properties") {
            let no_extra = matches!(s.get("additionalProperties"), Some(Object::Boolean(false)));
            for (k, v) in &value_as_hash(value).unwrap().borrow().entries {
                match properties.borrow().get(k) {
                    Some(sub) => {
                        let sub_path = format!("{}.{}", path, k);
                        errors.extend(validate_schema(sub, v, &sub_path)?);
                    }
                    None => {
                        if no_extra {
                            errors.push(format!("{}.{} is not allowed", path, k));
                        }
                    }
                }
            }
        }
    }
    if let Object::Array(arr) = value {
        if let Some(Object::Number(min)) = s.get("minItems") {
            if (arr.borrow().elements.len() as f64) < *min {
                errors.push(format!(
                    "{} must contain at least {} items",
                    path, *min as i64
                ));
            }
        }
        if let Some(Object::Number(max)) = s.get("maxItems") {
            if (arr.borrow().elements.len() as f64) > *max {
                errors.push(format!(
                    "{} must contain at most {} items",
                    path, *max as i64
                ));
            }
        }
        if let Some(items_schema) = s.get("items") {
            for (i, elem) in arr.borrow().elements.iter().enumerate() {
                let item_path = format!("{}[{}]", path, i);
                errors.extend(validate_schema(items_schema, elem, &item_path)?);
            }
        }
    }
    if let Object::String(st) = value {
        let len = st.len() as f64;
        if let Some(Object::Number(min)) = s.get("minLength") {
            if len < *min {
                errors.push(format!("{} length must be at least {}", path, *min as i64));
            }
        }
        if let Some(Object::Number(max)) = s.get("maxLength") {
            if len > *max {
                errors.push(format!("{} length must be at most {}", path, *max as i64));
            }
        }
    }
    if let Object::Number(n) = value {
        if let Some(Object::Number(min)) = s.get("minimum") {
            if *n < *min {
                errors.push(format!("{} must be >= {}", path, min));
            }
        }
        if let Some(Object::Number(max)) = s.get("maximum") {
            if *n > *max {
                errors.push(format!("{} must be <= {}", path, max));
            }
        }
    }
    Ok(errors)
}

pub(crate) fn type_matches(value: &Object, type_text: &str) -> bool {
    match type_text {
        "object" => matches!(value, Object::Hash(_)),
        "array" => matches!(value, Object::Array(_)),
        "string" => matches!(value, Object::String(_)),
        "number" => matches!(value, Object::Number(_)),
        "integer" => matches!(value, Object::Number(n) if n.fract() == 0.0),
        "boolean" => matches!(value, Object::Boolean(_)),
        "null" => matches!(value, Object::Null),
        _ => true,
    }
}

pub(crate) fn value_type_name(value: &Object) -> &'static str {
    match value {
        Object::Hash(_) => "object",
        Object::Array(_) => "array",
        Object::String(_) => "string",
        Object::Number(_) => "number",
        Object::Boolean(_) => "boolean",
        Object::Null => "null",
        Object::Undefined => "undefined",
        _ => "object",
    }
}

pub(crate) fn value_as_hash(value: &Object) -> Option<Rc<RefCell<HashData>>> {
    match value {
        Object::Hash(h) => Some(h.clone()),
        _ => None,
    }
}
