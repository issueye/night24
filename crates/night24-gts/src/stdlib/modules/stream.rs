use super::super::helpers::*;
use crate::object::{str_obj, CallContext, Object};

pub(crate) fn stream_module() -> Object {
    module(vec![(
        "fromString",
        native("stream.fromString", stream_from_string),
    )])
}

pub(crate) fn stream_from_string(ctx: &mut CallContext, args: &[Object]) -> Object {
    let text = match required_string(ctx, "stream.fromString", args, 0, "text") {
        Ok(v) => v,
        Err(e) => return e,
    };
    stream_from_text(text)
}

pub(crate) fn stream_from_text(text: String) -> Object {
    crate::object::http_stream::stream_from_text(text)
}

// ---------------------------------------------------------------------------
// exec: process execution module (@std/exec)
// ---------------------------------------------------------------------------

pub(crate) fn stream_from_text_object(text: String) -> Object {
    let stream = stream_from_text(text.clone());
    if let Object::Hash(h) = &stream {
        h.borrow_mut().set("text", str_obj(text));
    }
    stream
}
