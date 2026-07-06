use super::*;

pub(crate) fn value_to_object(value: &serde_json::Value) -> Object {
    match value {
        serde_json::Value::Null => Object::Null,
        serde_json::Value::Bool(b) => bool_obj(*b),
        serde_json::Value::Number(n) => {
            // Prefer integer representation when the value is integral, to
            // match the Go original's int64-then-f64 ordering.
            if let Some(i) = n.as_i64() {
                num_obj(i as f64)
            } else if let Some(u) = n.as_u64() {
                num_obj(u as f64)
            } else {
                num_obj(n.as_f64().unwrap_or(f64::NAN))
            }
        }
        serde_json::Value::String(s) => str_obj(s.clone()),
        serde_json::Value::Array(arr) => array(arr.iter().map(value_to_object).collect()),
        serde_json::Value::Object(map) => {
            let mut builder = ObjectBuilder::new();
            // Insert keys in sorted order for deterministic output, matching
            // the Go original's sortedStringKeys behavior.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                if let Some(v) = map.get(key) {
                    builder.insert(key.clone(), value_to_object(v));
                }
            }
            builder.build()
        }
    }
}

pub(crate) fn object_to_value(obj: &Object) -> serde_json::Value {
    match obj {
        Object::Null | Object::Undefined => serde_json::Value::Null,
        Object::Boolean(b) => serde_json::Value::Bool(*b),
        Object::Number(n) => {
            // Integer-valued numbers serialize as integers (preserved across
            // TOML/YAML round trips); otherwise as floats.
            if n.is_finite()
                && n.fract() == 0.0
                && (*n != 0.0 || n.is_sign_positive())
                && *n >= i64::MIN as f64
                && *n <= i64::MAX as f64
            {
                serde_json::Value::Number(serde_json::Number::from(*n as i64))
            } else {
                serde_json::Number::from_f64(*n)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            }
        }
        Object::String(s) => serde_json::Value::String(s.as_str().to_string()),
        Object::Array(arr) => {
            serde_json::Value::Array(arr.borrow().elements.iter().map(object_to_value).collect())
        }
        Object::Hash(hash) => {
            let mut map = serde_json::Map::new();
            for (k, v) in &hash.borrow().entries {
                map.insert(k.clone(), object_to_value(v));
            }
            serde_json::Value::Object(map)
        }
        // Non-data objects render to their inspect string for serialization,
        // matching the Go original's objectToGoValue fallback to Inspect().
        other => serde_json::Value::String(other.inspect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn value_to_object_converts_nested_json_data() {
        let obj = value_to_object(&json!({
            "z": [true, null, 3],
            "a": "text"
        }));

        let Object::Hash(hash) = obj else {
            panic!("expected hash object");
        };
        let hash = hash.borrow();

        assert_eq!(hash.entries[0].0, "a");
        assert_eq!(hash.entries[1].0, "z");
        assert!(matches!(hash.get("a"), Some(Object::String(value)) if value.as_str() == "text"));

        let Some(Object::Array(values)) = hash.get("z") else {
            panic!("expected array");
        };
        let values = values.borrow();
        assert!(matches!(values.elements[0], Object::Boolean(true)));
        assert!(matches!(values.elements[1], Object::Null));
        assert!(matches!(values.elements[2], Object::Number(value) if value == 3.0));
    }

    #[test]
    fn value_to_object_sorts_object_keys_deterministically() {
        let obj = value_to_object(&json!({
            "z": 1,
            "m": 2,
            "a": 3
        }));

        let Object::Hash(hash) = obj else {
            panic!("expected hash object");
        };
        let keys: Vec<String> = hash
            .borrow()
            .entries
            .iter()
            .map(|(key, _)| key.clone())
            .collect();

        assert_eq!(keys, vec!["a", "m", "z"]);
    }

    #[test]
    fn object_to_value_converts_runtime_data() {
        let hash = ObjectBuilder::new()
            .set("flag", bool_obj(true))
            .set("items", array(vec![str_obj("one"), Object::Undefined]))
            .into_shared();

        assert_eq!(
            object_to_value(&Object::Hash(hash)),
            json!({
                "flag": true,
                "items": ["one", null]
            })
        );
    }

    #[test]
    fn object_to_value_preserves_integer_and_float_number_shapes() {
        assert_eq!(object_to_value(&num_obj(42.0)), json!(42));
        assert_eq!(object_to_value(&num_obj(-7.0)), json!(-7));
        assert_eq!(object_to_value(&num_obj(1.25)), json!(1.25));
    }

    #[test]
    fn object_to_value_maps_non_finite_numbers_to_null() {
        assert_eq!(object_to_value(&num_obj(f64::NAN)), serde_json::Value::Null);
        assert_eq!(
            object_to_value(&num_obj(f64::INFINITY)),
            serde_json::Value::Null
        );
        assert_eq!(
            object_to_value(&num_obj(f64::NEG_INFINITY)),
            serde_json::Value::Null
        );
    }

    #[test]
    fn object_to_value_falls_back_to_inspect_for_non_data_objects() {
        let builtin = native("test.fn", |_ctx, _args| Object::Null);

        assert_eq!(object_to_value(&builtin), json!(builtin.inspect()));
    }
}
