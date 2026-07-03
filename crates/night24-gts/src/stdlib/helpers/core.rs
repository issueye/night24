use super::*;

pub(crate) fn module(entries: Vec<(&str, Object)>) -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    for (name, value) in entries {
        hash.borrow_mut().set(name, value);
    }
    Object::Hash(hash)
}

pub(crate) fn native(
    name: &str,
    func: impl Fn(&mut CallContext<'_>, &[Object]) -> Object + 'static,
) -> Object {
    Object::Builtin(Rc::new(Builtin {
        name: name.into(),
        func: Rc::new(func),
        extra: None,
    }))
}

pub(crate) fn object_to_text(value: &Object) -> String {
    match value {
        Object::String(value) => value.to_string(),
        Object::Undefined | Object::Null => String::new(),
        other => other.inspect(),
    }
}
