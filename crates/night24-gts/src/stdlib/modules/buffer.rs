use super::super::helpers::*;
use crate::object::{bool_obj, new_error, num_obj, CallContext, Object};

pub(crate) fn buffer_module() -> Object {
    module(vec![
        ("from", native("buffer.from", buffer_from)),
        ("alloc", native("buffer.alloc", buffer_alloc)),
        (
            "byteLength",
            native("buffer.byteLength", buffer_byte_length),
        ),
        ("concat", native("buffer.concat", buffer_concat)),
        ("isBuffer", native("buffer.isBuffer", buffer_is_buffer)),
    ])
}

pub(crate) fn buffer_from(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match args.first() {
        Some(v) => v,
        None => return new_error(ctx.pos.clone(), "buffer.from requires value"),
    };
    let encoding = match args.get(1) {
        Some(Object::String(s)) => normalize_encoding(s),
        Some(_) => return new_error(ctx.pos.clone(), "buffer.from: encoding must be a string"),
        None => EncodingKind::Utf8,
    };
    match decode_bytes(ctx, "buffer.from", value, encoding) {
        Ok(bytes) => make_buffer(bytes),
        Err(e) => e,
    }
}

pub(crate) fn buffer_alloc(ctx: &mut CallContext, args: &[Object]) -> Object {
    let size = match required_number(ctx, "buffer.alloc", args, 0, "size") {
        Ok(n) => n,
        Err(e) => return e,
    };
    if size < 0.0 {
        return new_error(ctx.pos.clone(), "buffer.alloc: size must be non-negative");
    }
    let size = size as usize;
    let fill = args.get(1);
    let fill_bytes = match fill {
        None | Some(Object::Undefined) => vec![0u8; size],
        Some(Object::Number(n)) => vec![((*n as i64) & 0xff) as u8; size.max(1)],
        Some(Object::String(s)) => {
            let b = s.as_bytes();
            if b.is_empty() {
                vec![0u8; size]
            } else {
                tile_bytes(b, size)
            }
        }
        Some(Object::Hash(_)) => match bytes_from_object(ctx, "buffer.alloc", fill.unwrap()) {
            Ok(b) if b.is_empty() => vec![0u8; size],
            Ok(b) => tile_bytes(&b, size),
            Err(e) => return e,
        },
        Some(_) => {
            return new_error(
                ctx.pos.clone(),
                "buffer.alloc: fill must be a number, string, or Buffer",
            )
        }
    };
    make_buffer(fill_bytes)
}

pub(crate) fn buffer_byte_length(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match args.first() {
        Some(v) => v,
        None => return new_error(ctx.pos.clone(), "buffer.byteLength requires value"),
    };
    let encoding = match args.get(1) {
        Some(Object::String(s)) => normalize_encoding(s),
        Some(_) => {
            return new_error(
                ctx.pos.clone(),
                "buffer.byteLength: encoding must be a string",
            )
        }
        None => EncodingKind::Utf8,
    };
    match decode_bytes(ctx, "buffer.byteLength", value, encoding) {
        Ok(bytes) => num_obj(bytes.len() as f64),
        Err(e) => e,
    }
}

pub(crate) fn buffer_concat(ctx: &mut CallContext, args: &[Object]) -> Object {
    let arr = match args.first() {
        Some(Object::Array(a)) => a.clone(),
        Some(_) => return new_error(ctx.pos.clone(), "buffer.concat: buffers must be an array"),
        None => return new_error(ctx.pos.clone(), "buffer.concat requires buffers"),
    };
    let mut out = Vec::new();
    for (i, elem) in arr.borrow().elements.iter().enumerate() {
        match bytes_from_object(ctx, "buffer.concat", elem) {
            Ok(b) => out.extend(b),
            Err(_) => {
                return new_error(
                    ctx.pos.clone(),
                    format!("buffer.concat: buffers[{}] must be a Buffer", i),
                )
            }
        }
    }
    make_buffer(out)
}

pub(crate) fn buffer_is_buffer(_ctx: &mut CallContext, args: &[Object]) -> Object {
    match args.first() {
        Some(Object::Hash(h)) => bool_obj(h.borrow().contains(BUFFER_DATA_KEY)),
        _ => bool_obj(false),
    }
}

pub(crate) enum EncodingKind {
    Utf8,
    Hex,
    Base64,
}

pub(crate) fn normalize_encoding(raw: &str) -> EncodingKind {
    let lower = raw.to_lowercase().replace('-', "");
    match lower.as_str() {
        "" | "utf8" | "utf" => EncodingKind::Utf8,
        "hex" => EncodingKind::Hex,
        "base64" => EncodingKind::Base64,
        _ => EncodingKind::Utf8,
    }
}

pub(crate) fn decode_bytes(
    ctx: &mut CallContext,
    name: &str,
    value: &Object,
    encoding: EncodingKind,
) -> Result<Vec<u8>, Object> {
    match (value, encoding) {
        (Object::String(s), EncodingKind::Utf8) => Ok(s.as_bytes().to_vec()),
        (Object::String(s), EncodingKind::Hex) => match hex_decode_bytes(name, s) {
            Ok(b) => Ok(b),
            Err(msg) => Err(new_error(ctx.pos.clone(), msg)),
        },
        (Object::String(s), EncodingKind::Base64) => {
            let table = base64_url_table();
            match base64_decode_into(&table, name, s, true) {
                Ok(b) => Ok(b),
                Err(msg) => Err(new_error(ctx.pos.clone(), msg)),
            }
        }
        (Object::Array(_), _) | (Object::Hash(_), _) => bytes_from_object(ctx, name, value),
        _ => Err(new_error(
            ctx.pos.clone(),
            format!("{}: value must be a string, array, or Buffer", name),
        )),
    }
}

// ---------------------------------------------------------------------------
// events: EventEmitter with on/once/off/emit (synchronous).
// ---------------------------------------------------------------------------
