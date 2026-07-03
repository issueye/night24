use super::*;

pub(crate) fn tile_bytes(src: &[u8], size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size);
    for i in 0..size {
        out.push(src[i % src.len()]);
    }
    out
}

pub(crate) const BUFFER_DATA_KEY: &str = "__buffer_data__";

pub(crate) fn bytes_from_object(
    ctx: &mut CallContext,
    name: &str,
    value: &Object,
) -> Result<Vec<u8>, Object> {
    match value {
        Object::String(s) => Ok(s.as_bytes().to_vec()),
        Object::Array(arr) => {
            let elements = &arr.borrow().elements;
            let mut out = Vec::with_capacity(elements.len());
            for (i, elem) in elements.iter().enumerate() {
                match elem {
                    Object::Number(n) => out.push(((*n as i64) & 0xff) as u8),
                    _ => {
                        return Err(new_error(
                            ctx.pos.clone(),
                            format!("{}: array item {} must be a number", name, i),
                        ))
                    }
                }
            }
            Ok(out)
        }
        Object::Hash(hash) => {
            if hash.borrow().contains(BUFFER_DATA_KEY) {
                match hash.borrow().get(BUFFER_DATA_KEY) {
                    Some(Object::Array(arr)) => {
                        let mut out = Vec::with_capacity(arr.borrow().elements.len());
                        for elem in &arr.borrow().elements {
                            match elem {
                                Object::Number(n) => out.push(((*n as i64) & 0xff) as u8),
                                _ => return Err(bytes_type_error(ctx, name)),
                            }
                        }
                        Ok(out)
                    }
                    _ => Err(bytes_type_error(ctx, name)),
                }
            } else {
                Err(bytes_type_error(ctx, name))
            }
        }
        _ => Err(bytes_type_error(ctx, name)),
    }
}

pub(crate) fn bytes_type_error(ctx: &mut CallContext, name: &str) -> Object {
    new_error(
        ctx.pos.clone(),
        format!("{}: value must be a string, array, or Buffer", name),
    )
}
