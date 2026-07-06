use super::super::helpers::*;
use crate::object::{bool_obj, new_error, str_obj, CallContext, Object};

pub(crate) fn json_module() -> Object {
    module(vec![
        ("parse5", native("json.parse5", json_parse5)),
        ("stringify5", native("json.stringify5", json_stringify5)),
        ("validate", native("json.validate", json_validate)),
        ("get", native("json.get", json_get)),
        ("set", native("json.set", json_set)),
        ("has", native("json.has", json_has)),
        ("remove", native("json.remove", json_remove)),
        ("patch", native("json.patch", json_patch)),
        ("diff", native("json.diff", json_diff)),
    ])
}

pub(crate) fn json_parse5(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "json.parse5", args);
    let text = match reader.required_string(0, "text") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let normalized = normalize_json5(&text);
    match simple_json_parse(&normalized) {
        Ok(value) => json_to_object(value),
        Err(err) => new_error(ctx.pos.clone(), format!("json.parse5: {}", err)),
    }
}

pub(crate) fn json_stringify5(ctx: &mut CallContext, args: &[Object]) -> Object {
    let Some(value) = args.first() else {
        return new_error(ctx.pos.clone(), "json.stringify5 requires value");
    };
    let (space, single_quote) = stringify_options(args.get(1));
    let mut result = object_to_json(value, 0, space.as_deref());
    if single_quote {
        result = result.replace('"', "'");
    }
    str_obj(result)
}

pub(crate) fn json_validate(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.len() < 2 {
        return new_error(ctx.pos.clone(), "json.validate requires data and schema");
    }
    let Object::Hash(schema) = &args[1] else {
        return new_error(ctx.pos.clone(), "json.validate expects hash schema");
    };
    let mut errors = Vec::new();
    validate_json_value(&args[0], &schema.borrow(), "", &mut errors);
    if errors.is_empty() {
        module(vec![("valid", bool_obj(true))])
    } else {
        module(vec![
            ("valid", bool_obj(false)),
            ("errors", array(errors.into_iter().map(str_obj).collect())),
        ])
    }
}

pub(crate) fn json_get(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "json.get", args);
    let path = match reader.required_string(1, "path") {
        Ok(value) => value,
        Err(err) => return err,
    };
    args.first()
        .and_then(|doc| pointer_get(doc, &path))
        .unwrap_or(Object::Undefined)
}

pub(crate) fn json_set(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.len() < 3 {
        return new_error(ctx.pos.clone(), "json.set requires doc, path, and value");
    }
    let reader = ArgReader::new(ctx, "json.set", args);
    let path = match reader.required_string(1, "path") {
        Ok(value) => value,
        Err(err) => return err,
    };
    pointer_set(&args[0], &path, args[2].clone());
    Object::Undefined
}

pub(crate) fn json_has(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "json.has", args);
    let path = match reader.required_string(1, "path") {
        Ok(value) => value,
        Err(err) => return err,
    };
    bool_obj(
        args.first()
            .and_then(|doc| pointer_get(doc, &path))
            .is_some(),
    )
}

pub(crate) fn json_remove(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "json.remove", args);
    let path = match reader.required_string(1, "path") {
        Ok(value) => value,
        Err(err) => return err,
    };
    if let Some(doc) = args.first() {
        pointer_remove(doc, &path);
    }
    Object::Undefined
}

pub(crate) fn json_patch(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.len() < 2 {
        return new_error(ctx.pos.clone(), "json.patch requires doc and operations");
    }
    let Object::Array(ops) = &args[1] else {
        return new_error(ctx.pos.clone(), "json.patch expects array of operations");
    };
    for op_obj in &ops.borrow().elements {
        let Object::Hash(op_hash) = op_obj else {
            continue;
        };
        let op_hash = op_hash.borrow();
        let op_type = hash_string(&op_hash, "op").unwrap_or_default();
        let path = hash_string(&op_hash, "path").unwrap_or_default();
        match op_type.as_str() {
            "add" | "replace" => {
                if let Some(value) = op_hash.get("value") {
                    pointer_set(&args[0], &path, value.clone());
                }
            }
            "remove" => pointer_remove(&args[0], &path),
            "move" => {
                let from = hash_string(&op_hash, "from").unwrap_or_default();
                if let Some(value) = pointer_get(&args[0], &from) {
                    pointer_remove(&args[0], &from);
                    pointer_set(&args[0], &path, value);
                }
            }
            "copy" => {
                let from = hash_string(&op_hash, "from").unwrap_or_default();
                if let Some(value) = pointer_get(&args[0], &from) {
                    pointer_set(&args[0], &path, deep_clone_object(&value));
                }
            }
            "test" => {
                let expected = op_hash.get("value").cloned().unwrap_or(Object::Undefined);
                let current = pointer_get(&args[0], &path).unwrap_or(Object::Undefined);
                if !objects_deep_equal(&current, &expected) {
                    return new_error(
                        ctx.pos.clone(),
                        format!("json.patch: test failed at {}", path),
                    );
                }
            }
            _ => {}
        }
    }
    Object::Undefined
}

pub(crate) fn json_diff(ctx: &mut CallContext, args: &[Object]) -> Object {
    if args.len() < 2 {
        return new_error(ctx.pos.clone(), "json.diff requires oldDoc and newDoc");
    }
    let mut patches = Vec::new();
    diff_objects(&args[0], &args[1], "", &mut patches);
    array(patches)
}
