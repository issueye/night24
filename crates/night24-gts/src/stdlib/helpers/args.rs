use super::*;

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn required_string(
    ctx: &CallContext,
    module: &str,
    args: &[Object],
    index: usize,
    name: &str,
) -> Result<String, Object> {
    match args.get(index) {
        Some(Object::String(value)) => Ok(value.to_string()),
        Some(_) => Err(new_error(
            ctx.pos.clone(),
            format!("{}: {} must be a string", module, name),
        )),
        None => Err(new_error(
            ctx.pos.clone(),
            format!("{} requires {}", module, name),
        )),
    }
}

pub(crate) fn required_number(
    ctx: &CallContext,
    module: &str,
    args: &[Object],
    index: usize,
    name: &str,
) -> Result<f64, Object> {
    match args.get(index) {
        Some(Object::Number(value)) => Ok(*value),
        Some(_) => Err(new_error(
            ctx.pos.clone(),
            format!("{}: {} must be a number", module, name),
        )),
        None => Err(new_error(
            ctx.pos.clone(),
            format!("{} requires {}", module, name),
        )),
    }
}

pub(crate) fn string_args(
    ctx: &CallContext,
    module: &str,
    args: &[Object],
) -> Result<Vec<String>, Object> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        match arg {
            Object::String(value) => out.push(value.to_string()),
            _ => {
                return Err(new_error(
                    ctx.pos.clone(),
                    format!("{}: all arguments must be strings", module),
                ))
            }
        }
    }
    Ok(out)
}

pub(crate) fn hash_string(hash: &HashData, key: &str) -> Option<String> {
    match hash.get(key) {
        Some(Object::String(value)) => Some(value.to_string()),
        _ => None,
    }
}

pub(crate) fn hash_bool_arg(value: Option<&Object>, key: &str) -> Option<bool> {
    match value {
        Some(Object::Hash(hash)) => match hash.borrow().get(key) {
            Some(Object::Boolean(value)) => Some(*value),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn array(elements: Vec<Object>) -> Object {
    Object::Array(Rc::new(RefCell::new(ArrayData { elements })))
}
