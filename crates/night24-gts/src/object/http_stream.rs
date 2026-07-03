//! Shared conversion helpers for async HTTP responses and text-backed streams.
//!
//! This module owns the single source of truth for turning
//! [`AsyncCompletionData`] / [`AsyncHttpResponse`] into language-level
//! [`Object`]s and for building a readable text stream (the `body` of an HTTP
//! stream response, or `@std/stream`'s `fromString`).
//!
//! It sits in the `object` layer (lower than `stdlib`) so that both the VM's
//! async-completion drain (`object::vm`) and the stdlib HTTP/stream modules can
//! share one implementation instead of carrying private duplicates.

use std::cell::RefCell;
use std::rc::Rc;

use crate::async_runtime::{AsyncCompletionData, AsyncHttpResponse};

use super::value::{
    bool_obj, new_error, num_obj, str_obj, ArrayData, CallContext, HashData, Object,
};

// ---------------------------------------------------------------------------
// AsyncCompletionData -> Object
// ---------------------------------------------------------------------------

/// Convert a drained async completion's owned data into an [`Object`].
///
/// This is the boundary where background-worker data re-enters the
/// single-threaded VM world. Called by `VirtualMachine::drain_async_completions`.
pub fn async_completion_data_to_object(data: AsyncCompletionData) -> Object {
    match data {
        AsyncCompletionData::Undefined => Object::Undefined,
        AsyncCompletionData::Text(text) | AsyncCompletionData::JsonText(text) => str_obj(text),
        AsyncCompletionData::Bytes(bytes) => Object::Array(Rc::new(RefCell::new(ArrayData {
            elements: bytes.into_iter().map(|byte| num_obj(byte as f64)).collect(),
        }))),
        AsyncCompletionData::HttpResponse(response) => http_response_to_object(response),
        AsyncCompletionData::HttpStreamResponse(response) => {
            http_stream_response_to_object(response)
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP response -> Object
// ---------------------------------------------------------------------------

/// Build the `{status, statusText, headers, body, ok}` object for a buffered
/// HTTP response (body as a string).
pub fn http_response_to_object(response: AsyncHttpResponse) -> Object {
    let obj = Rc::new(RefCell::new(HashData::default()));
    obj.borrow_mut()
        .set("status", num_obj(response.status as f64));
    obj.borrow_mut()
        .set("statusText", str_obj(response.status_text));
    obj.borrow_mut()
        .set("headers", headers_hash(&response.headers));
    let body = String::from_utf8_lossy(&response.body).into_owned();
    obj.borrow_mut().set("body", str_obj(body));
    obj.borrow_mut()
        .set("ok", bool_obj((200..300).contains(&response.status)));
    Object::Hash(obj)
}

/// Build the streamed variant: same envelope, but `body` is a readable text
/// stream (see [`stream_from_text`]) and a no-op `close`.
pub fn http_stream_response_to_object(response: AsyncHttpResponse) -> Object {
    let obj = Rc::new(RefCell::new(HashData::default()));
    obj.borrow_mut()
        .set("status", num_obj(response.status as f64));
    obj.borrow_mut()
        .set("statusText", str_obj(response.status_text));
    obj.borrow_mut()
        .set("headers", headers_hash(&response.headers));
    let body = String::from_utf8_lossy(&response.body).into_owned();
    obj.borrow_mut().set("body", stream_from_text(body));
    obj.borrow_mut()
        .set("ok", bool_obj((200..300).contains(&response.status)));
    obj.borrow_mut().set(
        "close",
        Object::Builtin(Rc::new(crate::object::value::Builtin {
            name: "http.streamAsync.close".to_string(),
            func: Rc::new(|_ctx, _args| Object::Undefined),
            extra: None,
        })),
    );
    Object::Hash(obj)
}

fn headers_hash(headers: &[(String, String)]) -> Object {
    let hash = Rc::new(RefCell::new(HashData::default()));
    for (name, value) in headers {
        hash.borrow_mut().set(name.clone(), str_obj(value.clone()));
    }
    Object::Hash(hash)
}

// ---------------------------------------------------------------------------
// Text stream (read / readText / readLine / readAll / close)
// ---------------------------------------------------------------------------

/// Mutable cursor over a text buffer, shared by the stream's reader closures.
pub(crate) struct StreamState {
    pub text: String,
    pub pos: usize,
    pub closed: bool,
}

/// Build a readable stream object over `text` with `read` / `readText` /
/// `readLine` / `readAll` / `close`. Also exposes the original text via a
/// `text` field (used by `@std/stream`).
pub fn stream_from_text(text: String) -> Object {
    let state = Rc::new(RefCell::new(StreamState {
        text: text.clone(),
        pos: 0,
        closed: false,
    }));
    let stream = Rc::new(RefCell::new(HashData::default()));
    stream.borrow_mut().set("text", str_obj(text));

    let s = state.clone();
    stream.borrow_mut().set(
        "read",
        Object::Builtin(Rc::new(crate::object::value::Builtin {
            name: "stream.read".to_string(),
            func: Rc::new(move |ctx, args| stream_read(ctx, args, &s)),
            extra: None,
        })),
    );

    let s = state.clone();
    stream.borrow_mut().set(
        "readText",
        Object::Builtin(Rc::new(crate::object::value::Builtin {
            name: "stream.readText".to_string(),
            func: Rc::new(move |ctx, args| stream_read_text(ctx, args, &s)),
            extra: None,
        })),
    );

    let s = state.clone();
    stream.borrow_mut().set(
        "readLine",
        Object::Builtin(Rc::new(crate::object::value::Builtin {
            name: "stream.readLine".to_string(),
            func: Rc::new(move |_ctx, _args| stream_read_line(&s)),
            extra: None,
        })),
    );

    let s = state.clone();
    stream.borrow_mut().set(
        "readAll",
        Object::Builtin(Rc::new(crate::object::value::Builtin {
            name: "stream.readAll".to_string(),
            func: Rc::new(move |_ctx, _args| stream_read_all(&s)),
            extra: None,
        })),
    );

    let s = state;
    stream.borrow_mut().set(
        "close",
        Object::Builtin(Rc::new(crate::object::value::Builtin {
            name: "stream.close".to_string(),
            func: Rc::new(move |_ctx, _args| {
                s.borrow_mut().closed = true;
                Object::Undefined
            }),
            extra: None,
        })),
    );

    Object::Hash(stream)
}

fn stream_size(ctx: &CallContext<'_>, name: &str, args: &[Object]) -> Result<usize, Object> {
    match args.first() {
        Some(Object::Number(n)) if *n > 0.0 => Ok(*n as usize),
        Some(Object::Number(_)) => Err(new_error(
            ctx.pos.clone(),
            format!("{name}: size must be positive"),
        )),
        Some(_) => Err(new_error(
            ctx.pos.clone(),
            format!("{name}: size must be a number"),
        )),
        None => Ok(8192),
    }
}

fn stream_read(
    ctx: &mut CallContext<'_>,
    args: &[Object],
    state: &Rc<RefCell<StreamState>>,
) -> Object {
    let size = match stream_size(ctx, "stream.read", args) {
        Ok(size) => size,
        Err(err) => return err,
    };
    let mut s = state.borrow_mut();
    if s.closed || s.pos >= s.text.len() {
        return Object::Null;
    }
    let end = (s.pos + size).min(s.text.len());
    let bytes = s.text.as_bytes()[s.pos..end].to_vec();
    s.pos = end;
    Object::Array(Rc::new(RefCell::new(ArrayData {
        elements: bytes.into_iter().map(|byte| num_obj(byte as f64)).collect(),
    })))
}

fn stream_read_text(
    ctx: &mut CallContext<'_>,
    args: &[Object],
    state: &Rc<RefCell<StreamState>>,
) -> Object {
    let size = match stream_size(ctx, "stream.readText", args) {
        Ok(size) => size,
        Err(err) => return err,
    };
    let mut s = state.borrow_mut();
    if s.closed || s.pos >= s.text.len() {
        return Object::Null;
    }
    let end = (s.pos + size).min(s.text.len());
    let chunk = s.text[s.pos..end].to_string();
    s.pos = end;
    str_obj(chunk)
}

fn stream_read_line(state: &Rc<RefCell<StreamState>>) -> Object {
    let mut s = state.borrow_mut();
    if s.closed || s.pos >= s.text.len() {
        return Object::Null;
    }
    let rest = &s.text[s.pos..];
    match rest.find('\n') {
        Some(idx) => {
            let line = rest[..idx].trim_end_matches('\r').to_string();
            s.pos += idx + 1;
            str_obj(line)
        }
        None => {
            let line = rest.trim_end_matches('\r').to_string();
            s.pos = s.text.len();
            str_obj(line)
        }
    }
}

fn stream_read_all(state: &Rc<RefCell<StreamState>>) -> Object {
    let s = state.borrow();
    if s.closed || s.pos >= s.text.len() {
        return str_obj(String::new());
    }
    str_obj(s.text[s.pos..].to_string())
}
