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
    let reader = ArgReader::new(ctx, "yaml.parse", args);
    let text = match reader.required_string(0, "text") {
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
    let reader = ArgReader::new(ctx, "yaml.readFileSync", args);
    let path = match reader.required_string(0, "path") {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Position;
    use crate::object::{bool_obj, num_obj, Environment, VirtualMachine};

    fn test_context(env: &crate::object::EnvRef) -> CallContext<'_> {
        CallContext::new(env, Position::default())
    }

    #[test]
    fn parse_converts_yaml_mapping_to_object() {
        let env = Environment::new_root(VirtualMachine::new());
        let mut ctx = test_context(&env);
        let parsed = yaml_parse(
            &mut ctx,
            &[str_obj(
                "name: night24\nversion: 24\nfeatures:\n  - yaml\n  - stdlib\n",
            )],
        );

        let Object::Hash(hash) = parsed else {
            panic!("expected hash object");
        };
        let hash = hash.borrow();

        assert!(
            matches!(hash.get("name"), Some(Object::String(value)) if value.as_str() == "night24")
        );
        assert!(matches!(hash.get("version"), Some(Object::Number(value)) if *value == 24.0));

        let Some(Object::Array(features)) = hash.get("features") else {
            panic!("expected features array");
        };
        let features = features.borrow();
        assert!(matches!(&features.elements[0], Object::String(value) if value.as_str() == "yaml"));
        assert!(
            matches!(&features.elements[1], Object::String(value) if value.as_str() == "stdlib")
        );
    }

    #[test]
    fn stringify_writes_basic_yaml_mapping() {
        let object = ObjectBuilder::new()
            .set("enabled", bool_obj(true))
            .set("name", str_obj("night24"))
            .set("version", num_obj(24.0))
            .build();

        let yaml = yaml_stringify_value(&object).unwrap();

        assert!(yaml.contains("enabled: true"));
        assert!(yaml.contains("name: night24"));
        assert!(yaml.contains("version: 24"));
    }
}

// ---------------------------------------------------------------------------
// xml: custom DOM with { name, attributes, children, text } nodes, matching
// the Go original's self-implemented parser/serializer.
// ---------------------------------------------------------------------------
