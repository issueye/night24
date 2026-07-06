use std::cell::RefCell;
use std::rc::Rc;

use crate::object::*;

use super::super::expressions::apply_function;
use super::FnPtr;

pub(super) fn map_global() -> Object {
    let map_constructor: FnPtr = Rc::new(|_ctx, args| {
        let map_data = Rc::new(RefCell::new(MapData::default()));
        if let Some(Object::Array(arr)) = args.first() {
            for entry in &arr.borrow().elements {
                if let Object::Array(pair) = entry {
                    let pair_data = pair.borrow();
                    if pair_data.elements.len() >= 2 {
                        let key = pair_data.elements[0].clone();
                        let value = pair_data.elements[1].clone();
                        map_data.borrow_mut().set(key, value);
                    }
                }
            }
        }
        Object::Map(map_data)
    });
    Object::Builtin(Rc::new(Builtin {
        name: "Map".into(),
        func: map_constructor,
        extra: None,
    }))
}

pub(super) fn set_global() -> Object {
    let set_constructor: FnPtr = Rc::new(|_ctx, args| {
        let set_data = Rc::new(RefCell::new(SetData::default()));
        if let Some(Object::Array(arr)) = args.first() {
            for value in &arr.borrow().elements {
                set_data.borrow_mut().add(value.clone());
            }
        }
        Object::Set(set_data)
    });
    Object::Builtin(Rc::new(Builtin {
        name: "Set".into(),
        func: set_constructor,
        extra: None,
    }))
}

pub fn map_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "set" => Some(map_set),
        "get" => Some(map_get),
        "has" => Some(map_has),
        "delete" => Some(map_delete),
        "clear" => Some(map_clear),
        "keys" => Some(map_keys),
        "values" => Some(map_values),
        "entries" => Some(map_entries),
        "forEach" => Some(map_for_each),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}

pub fn set_method(name: &str) -> Option<BuiltinFn> {
    let f: Option<fn(&mut CallContext, &[Object]) -> Object> = match name {
        "add" => Some(set_add),
        "has" => Some(set_has),
        "delete" => Some(set_delete),
        "clear" => Some(set_clear),
        "values" => Some(set_values),
        "entries" => Some(set_entries),
        "forEach" => Some(set_for_each),
        _ => None,
    };
    f.map(|f| Rc::new(f) as BuiltinFn)
}

fn active_map(ctx: &CallContext) -> Option<Rc<RefCell<MapData>>> {
    ctx.receiver.as_ref().and_then(|t| {
        if let Object::Map(m) = t {
            Some(Rc::clone(m))
        } else {
            None
        }
    })
}

fn map_set(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        if let Some(key) = args.first() {
            let value = args.get(1).cloned().unwrap_or(Object::Undefined);
            map.borrow_mut().set(key.clone(), value);
            return ctx.receiver.clone().unwrap_or(Object::Undefined);
        }
    }
    Object::Undefined
}

fn map_get(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        if let Some(key) = args.first() {
            return map.borrow().get(key).cloned().unwrap_or(Object::Undefined);
        }
    }
    Object::Undefined
}

fn map_has(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        if let Some(key) = args.first() {
            return Object::Boolean(map.borrow().has(key));
        }
    }
    Object::Boolean(false)
}

fn map_delete(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        if let Some(key) = args.first() {
            return Object::Boolean(map.borrow_mut().delete(key));
        }
    }
    Object::Boolean(false)
}

fn map_clear(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        map.borrow_mut().clear();
    }
    Object::Undefined
}

fn map_keys(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        let keys: Vec<Object> = map
            .borrow()
            .entries
            .iter()
            .map(|(_, k, _)| k.clone())
            .collect();
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: keys })));
    }
    Object::Array(Rc::new(RefCell::new(ArrayData::default())))
}

fn map_values(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        let values: Vec<Object> = map
            .borrow()
            .entries
            .iter()
            .map(|(_, _, v)| v.clone())
            .collect();
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: values })));
    }
    Object::Array(Rc::new(RefCell::new(ArrayData::default())))
}

fn map_entries(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        let entries: Vec<Object> = map
            .borrow()
            .entries
            .iter()
            .map(|(_, k, v)| {
                Object::Array(Rc::new(RefCell::new(ArrayData {
                    elements: vec![k.clone(), v.clone()],
                })))
            })
            .collect();
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: entries })));
    }
    Object::Array(Rc::new(RefCell::new(ArrayData::default())))
}

fn map_for_each(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(map) = active_map(ctx) {
        if let Some(callback) = args.first() {
            let entries = map.borrow().entries.clone();
            for (_, k, v) in entries {
                let _ = apply_function(callback, ctx.env, &[v, k], None, ctx.pos.clone());
            }
        }
    }
    Object::Undefined
}

fn active_set(ctx: &CallContext) -> Option<Rc<RefCell<SetData>>> {
    ctx.receiver.as_ref().and_then(|t| {
        if let Object::Set(s) = t {
            Some(Rc::clone(s))
        } else {
            None
        }
    })
}

fn set_add(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        if let Some(value) = args.first() {
            set.borrow_mut().add(value.clone());
            return ctx.receiver.clone().unwrap_or(Object::Undefined);
        }
    }
    Object::Undefined
}

fn set_has(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        if let Some(value) = args.first() {
            return Object::Boolean(set.borrow().has(value));
        }
    }
    Object::Boolean(false)
}

fn set_delete(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        if let Some(value) = args.first() {
            return Object::Boolean(set.borrow_mut().delete(value));
        }
    }
    Object::Boolean(false)
}

fn set_clear(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        set.borrow_mut().clear();
    }
    Object::Undefined
}

fn set_values(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        let values: Vec<Object> = set
            .borrow()
            .entries
            .iter()
            .map(|(_, v)| v.clone())
            .collect();
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: values })));
    }
    Object::Array(Rc::new(RefCell::new(ArrayData::default())))
}

fn set_entries(ctx: &mut CallContext, _args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        let entries: Vec<Object> = set
            .borrow()
            .entries
            .iter()
            .map(|(_, v)| {
                Object::Array(Rc::new(RefCell::new(ArrayData {
                    elements: vec![v.clone(), v.clone()],
                })))
            })
            .collect();
        return Object::Array(Rc::new(RefCell::new(ArrayData { elements: entries })));
    }
    Object::Array(Rc::new(RefCell::new(ArrayData::default())))
}

fn set_for_each(ctx: &mut CallContext, args: &[Object]) -> Object {
    if let Some(set) = active_set(ctx) {
        if let Some(callback) = args.first() {
            let entries = set.borrow().entries.clone();
            for (_, v) in entries {
                let _ = apply_function(callback, ctx.env, &[v.clone(), v], None, ctx.pos.clone());
            }
        }
    }
    Object::Undefined
}
