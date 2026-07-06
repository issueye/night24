use std::fs;

use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, Object};

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
    let reader = ArgReader::new(ctx, "toml.parse", args);
    let text = match reader.required_string(0, "text") {
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
    let reader = ArgReader::new(ctx, "toml.readFileSync", args);
    let path = match reader.required_string(0, "path") {
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
            let mut builder = ObjectBuilder::new();
            let mut keys: Vec<&String> = table.keys().collect();
            keys.sort();
            for key in keys {
                if let Some(v) = table.get(key) {
                    builder.insert(key.clone(), toml_to_object(v));
                }
            }
            builder.build()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn object_string_field(object: &Object, key: &str) -> String {
        let Object::Hash(hash) = object else {
            panic!("expected object");
        };
        match hash.borrow().get(key) {
            Some(Object::String(value)) => value.to_string(),
            _ => panic!("expected string field {key}"),
        }
    }

    fn object_number_field(object: &Object, key: &str) -> f64 {
        let Object::Hash(hash) = object else {
            panic!("expected object");
        };
        match hash.borrow().get(key) {
            Some(Object::Number(value)) => *value,
            _ => panic!("expected number field {key}"),
        }
    }

    #[test]
    fn toml_to_object_converts_table_fields() {
        let value = toml::from_str::<toml::Value>(
            r#"
name = "Night24"
count = 24
"#,
        )
        .unwrap();

        let object = toml_to_object(&value);

        assert_eq!(object_string_field(&object, "name"), "Night24");
        assert_eq!(object_number_field(&object, "count"), 24.0);
    }

    #[test]
    fn toml_stringify_value_writes_basic_table() {
        let object = ObjectBuilder::new()
            .set("name", str_obj("Night24"))
            .set("count", num_obj(24.0))
            .build();

        let text = toml_stringify_value(&object).unwrap();

        assert!(text.contains("name = \"Night24\""));
        assert!(text.contains("count = 24"));
    }
}

// ---------------------------------------------------------------------------
// yaml
// ---------------------------------------------------------------------------
