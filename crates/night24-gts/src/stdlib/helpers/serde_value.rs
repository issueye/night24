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
            let hash = Rc::new(RefCell::new(HashData::default()));
            // Insert keys in sorted order for deterministic output, matching
            // the Go original's sortedStringKeys behavior.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                if let Some(v) = map.get(key) {
                    hash.borrow_mut().set(key.clone(), value_to_object(v));
                }
            }
            Object::Hash(hash)
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
            if let Some(i) = serde_json::Number::from_f64(*n).and_then(|x| {
                if x.is_i64() || x.is_u64() {
                    Some(x)
                } else {
                    None
                }
            }) {
                serde_json::Value::Number(i)
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
