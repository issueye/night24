use super::super::helpers::*;
use crate::object::{new_error, str_obj, CallContext, Object};

pub(crate) fn hex_module() -> Object {
    module(vec![
        ("encode", native("hex.encode", hex_encode_fn)),
        ("decode", native("hex.decode", hex_decode_fn)),
    ])
}

pub(crate) fn hex_encode_fn(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match args.first() {
        Some(v) => v,
        None => return new_error(ctx.pos.clone(), "hex.encode requires value"),
    };
    match bytes_from_object(ctx, "hex.encode", value) {
        Ok(bytes) => str_obj(hex_encode_bytes(&bytes)),
        Err(err) => err,
    }
}

pub(crate) fn hex_decode_fn(ctx: &mut CallContext, args: &[Object]) -> Object {
    let text = match required_string(ctx, "hex.decode", args, 0, "text") {
        Ok(value) => value,
        Err(err) => return err,
    };
    match hex_decode_bytes("hex.decode", &text) {
        Ok(bytes) => bytes_result(ctx, "hex.decode", bytes, args.get(1)),
        Err(msg) => new_error(ctx.pos.clone(), msg),
    }
}

// ---------------------------------------------------------------------------
// hash: adler32, crc32 (IEEE), crc64 (ISO), fnv1a (64-bit).
// ---------------------------------------------------------------------------
