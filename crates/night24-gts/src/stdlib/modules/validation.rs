use super::super::helpers::*;
use crate::object::{bool_obj, format_number, new_error, str_obj, CallContext, HashData, Object};

pub(crate) fn validation_module() -> Object {
    module(vec![
        (
            "validate",
            native("validation.validate", validation_validate),
        ),
        (
            "required",
            native("validation.required", validation_required),
        ),
        ("type", native("validation.type", validation_type)),
        ("email", native("validation.email", validation_email)),
        ("min", native("validation.min", validation_min)),
        ("max", native("validation.max", validation_max)),
    ])
}

pub(crate) fn validation_validate(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.len() < 2 {
        return new_error(
            ctx.pos.clone(),
            "validation.validate requires value and rules",
        );
    }
    let Object::Hash(rules) = &args[1] else {
        return new_error(
            ctx.pos.clone(),
            "validation.validate: rules must be an object",
        );
    };
    let mut errors = Vec::new();
    validate_value(&args[0], &rules.borrow(), "value", &mut errors);
    validation_result(errors)
}

pub(crate) fn validation_required(_ctx: &mut CallContext, args: &[Object]) -> Object {
    bool_obj(args.first().map(is_present).unwrap_or(false))
}

pub(crate) fn validation_type(ctx: &mut CallContext, args: &[Object]) -> Object {
    let expected = match required_string(ctx, "validation.type", args, 1, "type") {
        Ok(expected) => expected,
        Err(err) => return err,
    };
    bool_obj(
        args.first()
            .map(|value| value_matches_type(value, &expected))
            .unwrap_or(false),
    )
}

pub(crate) fn validation_email(ctx: &mut CallContext, args: &[Object]) -> Object {
    match required_string(ctx, "validation.email", args, 0, "value") {
        Ok(value) => bool_obj(is_email(&value)),
        Err(err) => err,
    }
}

pub(crate) fn validation_min(ctx: &mut CallContext, args: &[Object]) -> Object {
    let min = match required_number(ctx, "validation.min", args, 1, "min") {
        Ok(min) => min,
        Err(err) => return err,
    };
    bool_obj(
        args.first()
            .map(|value| value_at_least(value, min))
            .unwrap_or(false),
    )
}

pub(crate) fn validation_max(ctx: &mut CallContext, args: &[Object]) -> Object {
    let max = match required_number(ctx, "validation.max", args, 1, "max") {
        Ok(max) => max,
        Err(err) => return err,
    };
    bool_obj(
        args.first()
            .map(|value| value_at_most(value, max))
            .unwrap_or(false),
    )
}

pub(crate) fn validate_value(
    value: &Object,
    rules: &HashData,
    path: &str,
    errors: &mut Vec<String>,
) {
    if matches!(rules.get("required"), Some(Object::Boolean(true))) && !is_present(value) {
        errors.push(format!("{} is required", path));
    }
    if let Some(Object::String(expected)) = rules.get("type") {
        if is_present(value) && !value_matches_type(value, expected) {
            errors.push(format!("{} must be {}", path, expected));
        }
    }
    if matches!(rules.get("email"), Some(Object::Boolean(true))) {
        match value {
            Object::String(s) if is_email(s) => {}
            _ if is_present(value) => errors.push(format!("{} must be a valid email", path)),
            _ => {}
        }
    }
    if let Some(Object::Number(min)) = rules.get("min") {
        if is_present(value) && !value_at_least(value, *min) {
            errors.push(format!("{} must be at least {}", path, format_number(*min)));
        }
    }
    if let Some(Object::Number(max)) = rules.get("max") {
        if is_present(value) && !value_at_most(value, *max) {
            errors.push(format!("{} must be at most {}", path, format_number(*max)));
        }
    }
    if let Some(Object::Hash(fields)) = rules.get("fields") {
        if let Object::Hash(value_hash) = value {
            let value_hash = value_hash.borrow();
            for (key, field_rules) in &fields.borrow().entries {
                if let Object::Hash(rule_hash) = field_rules {
                    let field_value = value_hash.get(key).cloned().unwrap_or(Object::Undefined);
                    validate_value(&field_value, &rule_hash.borrow(), key, errors);
                }
            }
        } else if is_present(value) {
            errors.push(format!("{} must be object", path));
        }
    }
}

pub(crate) fn validation_result(errors: Vec<String>) -> Object {
    if errors.is_empty() {
        module(vec![
            ("valid", bool_obj(true)),
            ("errors", array(Vec::new())),
        ])
    } else {
        module(vec![
            ("valid", bool_obj(false)),
            ("errors", array(errors.into_iter().map(str_obj).collect())),
        ])
    }
}

pub(crate) fn is_present(value: &Object) -> bool {
    match value {
        Object::Undefined | Object::Null => false,
        Object::String(s) => !s.is_empty(),
        _ => true,
    }
}

pub(crate) fn value_matches_type(value: &Object, expected: &str) -> bool {
    match expected {
        "array" => matches!(value, Object::Array(_)),
        "object" => matches!(value, Object::Hash(_)),
        "date" => matches!(value, Object::Date(_)),
        other => value.type_tag() == other,
    }
}

pub(crate) fn is_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

pub(crate) fn value_at_least(value: &Object, min: f64) -> bool {
    match value {
        Object::Number(n) => *n >= min,
        Object::String(s) => s.chars().count() as f64 >= min,
        Object::Array(arr) => arr.borrow().elements.len() as f64 >= min,
        _ => false,
    }
}

pub(crate) fn value_at_most(value: &Object, max: f64) -> bool {
    match value {
        Object::Number(n) => *n <= max,
        Object::String(s) => s.chars().count() as f64 <= max,
        Object::Array(arr) => arr.borrow().elements.len() as f64 <= max,
        _ => false,
    }
}

// ===========================================================================
// P7 stdlib batch: toml / yaml / xml / markdown / schema / test / archive/zip.
//
// Codec modules (toml/yaml/xml) share a parse/stringify/readFileSync/
// writeFileSync surface and bridge through serde_json::Value, matching the Go
// originals' goValueToObject/objectToGoValue contracts (map keys sorted for
// determinism, integer-valued Numbers preserved as integers where possible).
// ===========================================================================

// ---------------------------------------------------------------------------
// serde_json::Value <-> Object bridge (shared by the codec modules).
// ---------------------------------------------------------------------------
