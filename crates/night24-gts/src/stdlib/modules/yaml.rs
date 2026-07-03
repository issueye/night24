use std::fs;

use super::super::helpers::*;
use crate::object::{new_error, str_obj, CallContext, Object};

pub(crate) fn yaml_module() -> Object {
    module(vec![
        ("parse", native("yaml.parse", yaml_parse)),
        ("stringify", native("yaml.stringify", yaml_stringify)),
        ("readFileSync", native("yaml.readFileSync", yaml_read_file)),
        (
            "writeFileSync",
            native("yaml.writeFileSync", yaml_write_file),
        ),
    ])
}

pub(crate) fn yaml_stringify_value(value: &Object) -> Result<String, String> {
    let v = object_to_value(value);
    serde_yaml::to_string(&v).map_err(|e| format!("yaml.stringify: {}", e))
}

pub(crate) fn yaml_parse(ctx: &mut CallContext, args: &[Object]) -> Object {
    let text = match required_string(ctx, "yaml.parse", args, 0, "text") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match serde_yaml::from_str::<serde_json::Value>(&text) {
        Ok(value) => value_to_object(&value),
        Err(e) => new_error(ctx.pos.clone(), format!("yaml.parse: {}", e)),
    }
}

pub(crate) fn yaml_stringify(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match args.first() {
        Some(v) => v,
        None => return new_error(ctx.pos.clone(), "yaml.stringify requires a value"),
    };
    match yaml_stringify_value(value) {
        Ok(s) => str_obj(s),
        Err(msg) => new_error(ctx.pos.clone(), msg),
    }
}

pub(crate) fn yaml_read_file(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "yaml.readFileSync", args, 0, "path") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match fs::read_to_string(&path) {
        Ok(text) => match serde_yaml::from_str::<serde_json::Value>(&text) {
            Ok(value) => value_to_object(&value),
            Err(e) => new_error(ctx.pos.clone(), format!("yaml.parse: {}", e)),
        },
        Err(e) => new_error(ctx.pos.clone(), format!("yaml.readFileSync: {}", e)),
    }
}

pub(crate) fn yaml_write_file(ctx: &mut CallContext, args: &[Object]) -> Object {
    codec_write_file(ctx, "yaml", args, "value", yaml_stringify_value)
}

// ---------------------------------------------------------------------------
// xml: custom DOM with { name, attributes, children, text } nodes, matching
// the Go original's self-implemented parser/serializer.
// ---------------------------------------------------------------------------
