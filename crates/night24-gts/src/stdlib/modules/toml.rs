use std::cell::RefCell;
use std::fs;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, HashData, Object};

pub(crate) fn toml_module() -> Object {
    module(vec![
        ("parse", native("toml.parse", toml_parse)),
        ("stringify", native("toml.stringify", toml_stringify)),
        ("readFileSync", native("toml.readFileSync", toml_read_file)),
        (
            "writeFileSync",
            native("toml.writeFileSync", toml_write_file),
        ),
    ])
}

pub(crate) fn toml_stringify_value(value: &Object) -> Result<String, String> {
    let tv = object_to_toml(value);
    toml::to_string(&tv).map_err(|e| format!("toml.stringify: {}", e))
}

pub(crate) fn toml_parse(ctx: &mut CallContext, args: &[Object]) -> Object {
    let text = match required_string(ctx, "toml.parse", args, 0, "text") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match toml::from_str::<toml::Value>(&text) {
        Ok(value) => toml_to_object(&value),
        Err(e) => new_error(ctx.pos.clone(), format!("toml.parse: {}", e)),
    }
}

pub(crate) fn toml_stringify(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match args.first() {
        Some(v) => v,
        None => return new_error(ctx.pos.clone(), "toml.stringify requires a value"),
    };
    match toml_stringify_value(value) {
        Ok(s) => str_obj(s),
        Err(msg) => new_error(ctx.pos.clone(), msg),
    }
}

pub(crate) fn toml_read_file(ctx: &mut CallContext, args: &[Object]) -> Object {
    let path = match required_string(ctx, "toml.readFileSync", args, 0, "path") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<toml::Value>(&text) {
            Ok(value) => toml_to_object(&value),
            Err(e) => new_error(ctx.pos.clone(), format!("toml.parse: {}", e)),
        },
        Err(e) => new_error(ctx.pos.clone(), format!("toml.readFileSync: {}", e)),
    }
}

pub(crate) fn toml_write_file(ctx: &mut CallContext, args: &[Object]) -> Object {
    codec_write_file(ctx, "toml", args, "value", toml_stringify_value)
}

pub(crate) fn toml_to_object(value: &toml::Value) -> Object {
    match value {
        toml::Value::String(s) => str_obj(s.clone()),
        toml::Value::Integer(i) => num_obj(*i as f64),
        toml::Value::Float(f) => num_obj(*f),
        toml::Value::Boolean(b) => bool_obj(*b),
        toml::Value::Datetime(dt) => str_obj(dt.to_string()),
        toml::Value::Array(arr) => array(arr.iter().map(toml_to_object).collect()),
        toml::Value::Table(table) => {
            let hash = Rc::new(RefCell::new(HashData::default()));
            let mut keys: Vec<&String> = table.keys().collect();
            keys.sort();
            for key in keys {
                if let Some(v) = table.get(key) {
                    hash.borrow_mut().set(key.clone(), toml_to_object(v));
                }
            }
            Object::Hash(hash)
        }
    }
}

pub(crate) fn object_to_toml(value: &Object) -> toml::Value {
    match value {
        Object::Null | Object::Undefined => toml::Value::String(String::new()),
        Object::Boolean(b) => toml::Value::Boolean(*b),
        Object::Number(n) => {
            if n.fract() == 0.0 && n.is_finite() {
                toml::Value::Integer(*n as i64)
            } else {
                toml::Value::Float(*n)
            }
        }
        Object::String(s) => toml::Value::String(s.as_str().to_string()),
        Object::Array(arr) => {
            toml::Value::Array(arr.borrow().elements.iter().map(object_to_toml).collect())
        }
        Object::Hash(hash) => {
            let mut table = toml::value::Table::new();
            for (k, v) in &hash.borrow().entries {
                table.insert(k.clone(), object_to_toml(v));
            }
            toml::Value::Table(table)
        }
        other => toml::Value::String(other.inspect()),
    }
}

// ---------------------------------------------------------------------------
// yaml
// ---------------------------------------------------------------------------
