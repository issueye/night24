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

pub(crate) fn required_callable(
    ctx: &CallContext,
    module: &str,
    args: &[Object],
    index: usize,
    name: &str,
) -> Result<Object, Object> {
    match args.get(index) {
        Some(value) if is_callable(value) => Ok(value.clone()),
        Some(_) => Err(new_error(
            ctx.pos.clone(),
            format!("{}: {} must be a function", module, name),
        )),
        None => Err(new_error(
            ctx.pos.clone(),
            format!("{} requires {}", module, name),
        )),
    }
}

pub(crate) fn is_callable(value: &Object) -> bool {
    matches!(
        value,
        Object::Function(_) | Object::Builtin(_) | Object::Closure(_)
    )
}

pub(crate) struct ArgReader<'a, 'ctx> {
    ctx: &'a CallContext<'ctx>,
    module: &'a str,
    args: &'a [Object],
}

impl<'a, 'ctx> ArgReader<'a, 'ctx> {
    pub(crate) fn new(ctx: &'a CallContext<'ctx>, module: &'a str, args: &'a [Object]) -> Self {
        Self { ctx, module, args }
    }

    pub(crate) fn required_string(&self, index: usize, name: &str) -> Result<String, Object> {
        required_string(self.ctx, self.module, self.args, index, name)
    }

    pub(crate) fn required_number(&self, index: usize, name: &str) -> Result<f64, Object> {
        required_number(self.ctx, self.module, self.args, index, name)
    }

    pub(crate) fn required_callable(&self, index: usize, name: &str) -> Result<Object, Object> {
        required_callable(self.ctx, self.module, self.args, index, name)
    }

    pub(crate) fn object_view(&self, index: usize) -> Option<std::cell::Ref<'a, HashData>> {
        match self.args.get(index) {
            Some(Object::Hash(hash)) => Some(hash.borrow()),
            _ => None,
        }
    }
}

pub(crate) struct ObjectView<'a> {
    hash: &'a HashData,
}

impl<'a> ObjectView<'a> {
    pub(crate) fn new(hash: &'a HashData) -> Self {
        Self { hash }
    }

    pub(crate) fn string(&self, key: &str) -> Option<String> {
        match self.hash.get(key) {
            Some(Object::String(value)) => Some(value.to_string()),
            Some(Object::Null | Object::Undefined) | None => None,
            Some(value) => Some(value_to_string(value)),
        }
    }

    pub(crate) fn bool(&self, key: &str) -> Option<bool> {
        match self.hash.get(key) {
            Some(Object::Boolean(value)) => Some(*value),
            Some(Object::Null | Object::Undefined) | None => None,
            Some(value) => Some(value.is_truthy()),
        }
    }

    pub(crate) fn number(&self, key: &str) -> Option<f64> {
        match self.hash.get(key) {
            Some(Object::Number(value)) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn object(&self, key: &str) -> Option<Object> {
        self.hash.get(key).cloned()
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
        Some(Object::Hash(hash)) => ObjectView::new(&hash.borrow()).bool(key),
        _ => None,
    }
}

pub(crate) fn array(elements: Vec<Object>) -> Object {
    Object::Array(Rc::new(RefCell::new(ArrayData { elements })))
}
