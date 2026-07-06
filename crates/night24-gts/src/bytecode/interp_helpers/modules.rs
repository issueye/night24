use crate::ast::Position;
use crate::object::{new_error, EnvRef, Object};

use super::super::chunk::Chunk;
use super::{read_string_operand, stack_underflow};

fn import_module_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    source: &str,
    pos: Position,
) -> Result<(), Object> {
    let importer = env.borrow().vm.importer();
    let module = match importer {
        Some(importer) => importer(env, source)?,
        None => {
            return Err(new_error(
                pos,
                "ImportError: module loading is not configured",
            ));
        }
    };
    stack.push(module);
    Ok(())
}

pub(in crate::bytecode) fn import_module_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    env: &EnvRef,
) -> Result<(), Object> {
    let (source, pos) = read_string_operand(chunk, ip, "IMPORT_MODULE")?;
    import_module_stack(stack, env, &source, pos)
}

fn export_name_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    name: String,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let exports = env.borrow().get("exports").unwrap_or(Object::Undefined);
    match exports {
        Object::Hash(h) => {
            h.borrow_mut().set(name, value);
            Ok(())
        }
        other => Err(new_error(
            pos,
            format!("TypeError: cannot export from {}", other.type_tag()),
        )),
    }
}

pub(in crate::bytecode) fn export_name_from_operand(
    chunk: &Chunk,
    ip: &mut usize,
    stack: &mut Vec<Object>,
    env: &EnvRef,
) -> Result<(), Object> {
    let (name, pos) = read_string_operand(chunk, ip, "EXPORT_NAME")?;
    export_name_stack(stack, env, name, pos)
}

pub(in crate::bytecode) fn export_all_stack(
    stack: &mut Vec<Object>,
    env: &EnvRef,
    pos: Position,
) -> Result<(), Object> {
    let source_exports = stack.pop().ok_or_else(|| stack_underflow(pos.clone()))?;
    let current_exports = env.borrow().get("exports").unwrap_or(Object::Undefined);
    match (&source_exports, &current_exports) {
        (Object::Hash(src), Object::Hash(dst)) => {
            let pairs: Vec<(String, Object)> = {
                let sb = src.borrow();
                sb.entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            };
            for (k, v) in pairs {
                // `export *` does NOT re-export a `default` binding.
                if k == "default" {
                    continue;
                }
                dst.borrow_mut().set(k, v);
            }
            Ok(())
        }
        (other_src, _) => Err(new_error(
            pos,
            format!(
                "TypeError: export * source must be a module object, got {}",
                other_src.type_tag()
            ),
        )),
    }
}

pub(in crate::bytecode) fn wrap_resolved_promise_stack(
    stack: &mut Vec<Object>,
    pos: Position,
) -> Result<(), Object> {
    let value = stack.pop().ok_or_else(|| stack_underflow(pos))?;
    let promise = crate::object::Promise::new();
    promise.resolve(value);
    stack.push(Object::Promise(promise));
    Ok(())
}
