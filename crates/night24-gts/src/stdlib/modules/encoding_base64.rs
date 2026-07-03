use super::super::helpers::*;
use crate::object::{new_error, str_obj, CallContext, Object};

pub(crate) fn base64_module() -> Object {
    module(vec![
        ("encode", native("base64.encode", base64_encode)),
        ("decode", native("base64.decode", base64_decode)),
        ("encodeURL", native("base64.encodeURL", base64_encode_url)),
        ("decodeURL", native("base64.decodeURL", base64_decode_url)),
    ])
}

pub(crate) fn base64_encode(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match args.first() {
        Some(v) => v,
        None => return new_error(ctx.pos.clone(), "base64.encode requires value"),
    };
    match bytes_from_object(ctx, "base64.encode", value) {
        Ok(bytes) => str_obj(base64_std_encode(&bytes)),
        Err(err) => err,
    }
}

pub(crate) fn base64_decode(ctx: &mut CallContext, args: &[Object]) -> Object {
    let text = match required_string(ctx, "base64.decode", args, 0, "text") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let table = base64_std_table();
    match base64_decode_into(&table, "base64.decode", &text, true) {
        Ok(bytes) => bytes_result(ctx, "base64.decode", bytes, args.get(1)),
        Err(msg) => new_error(ctx.pos.clone(), msg),
    }
}

pub(crate) fn base64_encode_url(ctx: &mut CallContext, args: &[Object]) -> Object {
    let value = match args.first() {
        Some(v) => v,
        None => return new_error(ctx.pos.clone(), "base64.encodeURL requires value"),
    };
    match bytes_from_object(ctx, "base64.encodeURL", value) {
        Ok(bytes) => str_obj(base64_url_encode(&bytes)),
        Err(err) => err,
    }
}

pub(crate) fn base64_decode_url(ctx: &mut CallContext, args: &[Object]) -> Object {
    let text = match required_string(ctx, "base64.decodeURL", args, 0, "text") {
        Ok(value) => value,
        Err(err) => return err,
    };
    let table = base64_url_table();
    match base64_decode_into(&table, "base64.decodeURL", &text, true) {
        Ok(bytes) => bytes_result(ctx, "base64.decodeURL", bytes, args.get(1)),
        Err(msg) => new_error(ctx.pos.clone(), msg),
    }
}
