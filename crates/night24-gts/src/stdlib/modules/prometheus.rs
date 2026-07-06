use super::super::helpers::*;
use crate::object::{num_obj, str_obj, CallContext, Object};

pub(crate) const PROMETHEUS_STATE_KEY: &str = "__prometheus_state__";

pub(crate) fn prometheus_module() -> Object {
    module(vec![(
        "create",
        native("prometheus.create", prometheus_create),
    )])
}

pub(crate) fn prometheus_create(_ctx: &mut CallContext, _args: &[Object]) -> Object {
    // metrics: Hash mapping name -> Number
    let metrics = ObjectBuilder::new().into_shared();
    let instance = ObjectBuilder::new()
        .set(PROMETHEUS_STATE_KEY, Object::Hash(metrics.clone()))
        .into_shared();

    let m = metrics.clone();
    instance.borrow_mut().set(
        "inc",
        native("prometheus.inc", move |ctx, args| {
            let reader = ArgReader::new(ctx, "prometheus.inc", args);
            let name = match reader.required_string(0, "name") {
                Ok(n) => n,
                Err(e) => return e,
            };
            let mut g = m.borrow_mut();
            let current = match g.get(&name) {
                Some(Object::Number(n)) => *n,
                _ => 0.0,
            };
            g.set(name, num_obj(current + 1.0));
            Object::Undefined
        }),
    );

    let m = metrics.clone();
    instance.borrow_mut().set(
        "set",
        native("prometheus.set", move |ctx, args| {
            let reader = ArgReader::new(ctx, "prometheus.set", args);
            let name = match reader.required_string(0, "name") {
                Ok(n) => n,
                Err(e) => return e,
            };
            let value = match reader.required_number(1, "value") {
                Ok(v) => v,
                Err(e) => return e,
            };
            m.borrow_mut().set(name, num_obj(value));
            Object::Undefined
        }),
    );

    let m = metrics.clone();
    instance.borrow_mut().set(
        "get",
        native("prometheus.get", move |ctx, args| {
            let reader = ArgReader::new(ctx, "prometheus.get", args);
            let name = match reader.required_string(0, "name") {
                Ok(n) => n,
                Err(e) => return e,
            };
            match m.borrow().get(&name).cloned() {
                Some(Object::Number(n)) => num_obj(n),
                _ => num_obj(0.0),
            }
        }),
    );

    let m = metrics.clone();
    instance.borrow_mut().set(
        "snapshot",
        native("prometheus.snapshot", move |_ctx, _args| {
            let g = m.borrow();
            let mut entries: Vec<Object> = Vec::with_capacity(g.entries.len());
            for (k, v) in &g.entries {
                entries.push(
                    ObjectBuilder::new()
                        .set("name", str_obj(k.clone()))
                        .set("value", v.clone())
                        .build(),
                );
            }
            array(entries)
        }),
    );

    Object::Hash(instance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{EnvRef, Environment, VirtualMachine};

    fn create_instance(env: &EnvRef) -> Object {
        let mut ctx = CallContext::new(env, Default::default());
        prometheus_create(&mut ctx, &[])
    }

    fn call_method(instance: &Object, env: &EnvRef, name: &str, args: &[Object]) -> Object {
        let method = match instance {
            Object::Hash(hash) => hash
                .borrow()
                .get(name)
                .cloned()
                .unwrap_or_else(|| panic!("missing prometheus method {name}")),
            other => panic!("expected prometheus instance, got {}", other.inspect()),
        };
        let builtin = match method {
            Object::Builtin(builtin) => builtin,
            other => panic!("expected builtin method {name}, got {}", other.inspect()),
        };
        let mut ctx = CallContext::new(env, Default::default());
        (builtin.func)(&mut ctx, args)
    }

    fn assert_number(value: Object, expected: f64) {
        match value {
            Object::Number(actual) => assert_eq!(actual, expected),
            other => panic!("expected number {expected}, got {}", other.inspect()),
        }
    }

    fn snapshot_entries(value: Object) -> Vec<(String, f64)> {
        match value {
            Object::Array(array) => array
                .borrow()
                .elements
                .iter()
                .map(|entry| match entry {
                    Object::Hash(hash) => {
                        let hash = hash.borrow();
                        let name = match hash.get("name") {
                            Some(Object::String(name)) => name.to_string(),
                            other => panic!("expected snapshot name string, got {other:?}"),
                        };
                        let value = match hash.get("value") {
                            Some(Object::Number(value)) => *value,
                            other => panic!("expected snapshot value number, got {other:?}"),
                        };
                        (name, value)
                    }
                    other => panic!("expected snapshot entry object, got {}", other.inspect()),
                })
                .collect(),
            other => panic!("expected snapshot array, got {}", other.inspect()),
        }
    }

    #[test]
    fn inc_set_get_round_trip_metric_values() {
        let env = Environment::new_root(VirtualMachine::new());
        let instance = create_instance(&env);

        assert_number(
            call_method(&instance, &env, "get", &[str_obj("requests_total")]),
            0.0,
        );

        assert_eq!(
            call_method(
                &instance,
                &env,
                "set",
                &[str_obj("requests_total"), num_obj(2.5)]
            ),
            Object::Undefined
        );
        assert_eq!(
            call_method(&instance, &env, "inc", &[str_obj("requests_total")]),
            Object::Undefined
        );

        assert_number(
            call_method(&instance, &env, "get", &[str_obj("requests_total")]),
            3.5,
        );
    }

    #[test]
    fn snapshot_returns_metric_entries() {
        let env = Environment::new_root(VirtualMachine::new());
        let instance = create_instance(&env);

        call_method(&instance, &env, "set", &[str_obj("alpha"), num_obj(2.0)]);
        call_method(&instance, &env, "inc", &[str_obj("beta")]);

        let entries = snapshot_entries(call_method(&instance, &env, "snapshot", &[]));
        assert_eq!(
            entries,
            vec![("alpha".to_string(), 2.0), ("beta".to_string(), 1.0)]
        );
    }
}

// ---------------------------------------------------------------------------
// highlight: terminal syntax highlighting subset (@std/highlight)
// ---------------------------------------------------------------------------
