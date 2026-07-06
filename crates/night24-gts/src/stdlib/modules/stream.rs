use super::super::helpers::*;
use crate::object::{str_obj, CallContext, Object};

pub(crate) fn stream_module() -> Object {
    module(vec![(
        "fromString",
        native("stream.fromString", stream_from_string),
    )])
}

pub(crate) fn stream_from_string(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "stream.fromString", args);
    let text = match reader.required_string(0, "text") {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn object_string_field(object: &Object, key: &str) -> String {
        let Object::Hash(hash) = object else {
            panic!("expected stream object");
        };
        match hash.borrow().get(key) {
            Some(Object::String(value)) => value.to_string(),
            _ => panic!("expected string field {key}"),
        }
    }

    #[test]
    fn stream_from_text_exposes_original_text() {
        let stream = stream_from_text("night24".to_string());

        assert_eq!(object_string_field(&stream, "text"), "night24");
    }
}
