use super::*;
use std::io::Write;
use std::time::{Duration, Instant};

/// Accumulated response state, mutated by web/http handlers via closures.
#[derive(Default)]
pub(crate) struct HttpResponseState {
    pub(crate) status: Option<u16>,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) content_type: Option<String>,
    pub(crate) body: Option<Vec<u8>>,
    /// When `Some`, the response is in **streaming mode** (chunked transfer
    /// encoding): `res.write()` sends data immediately to the TCP writer
    /// instead of buffering. Set by `res.begin()` (D1.1).
    pub(crate) stream_writer: Option<Rc<RefCell<Box<dyn Write>>>>,
    /// Owned request slot used by `@std/web` to let `res.begin()` consume the
    /// response writer before the framework's final buffered response path.
    pub(crate) request_slot: Option<Rc<RefCell<Option<tiny_http::Request>>>>,
    pub(crate) stream_closed: bool,
    pub(crate) stream_timeout: Option<Duration>,
    pub(crate) stream_deadline: Option<Instant>,
}

pub(crate) fn http_response_object(state: Rc<RefCell<HttpResponseState>>) -> Object {
    let obj = Rc::new(RefCell::new(HashData::default()));

    let s = state.clone();
    let out = obj.clone();
    obj.borrow_mut().set(
        "status",
        native("response.status", move |_ctx, args| {
            if let Some(Object::Number(n)) = args.first() {
                s.borrow_mut().status = Some(*n as u16);
            }
            Object::Hash(out.clone())
        }),
    );
    let s = state.clone();
    obj.borrow_mut().set(
        "setHeader",
        native("response.setHeader", move |_ctx, args| {
            let key = match args.first() {
                Some(Object::String(v)) => v.to_string(),
                Some(o) => o.inspect(),
                None => return Object::Undefined,
            };
            let value = match args.get(1) {
                Some(Object::String(v)) => v.to_string(),
                Some(o) => o.inspect(),
                None => return Object::Undefined,
            };
            if key.eq_ignore_ascii_case("content-type") {
                s.borrow_mut().content_type = Some(value);
            } else {
                s.borrow_mut().headers.push((key, value));
            }
            Object::Undefined
        }),
    );
    let s = state.clone();
    let out = obj.clone();
    obj.borrow_mut().set(
        "begin",
        native("response.begin", move |ctx, _args| {
            let (slot, status_code, content_type, headers, timeout) = {
                let g = s.borrow();
                if g.stream_writer.is_some() {
                    return Object::Hash(out.clone());
                }
                if g.stream_closed {
                    return new_error(
                        ctx.pos.clone(),
                        "response.begin: response has already been sent",
                    );
                }
                let Some(slot) = g.request_slot.as_ref().cloned() else {
                    return new_error(
                        ctx.pos.clone(),
                        "response.begin: streaming is not available for this response",
                    );
                };
                (
                    slot,
                    g.status.unwrap_or(200),
                    g.content_type
                        .clone()
                        .unwrap_or_else(|| "text/plain".to_string()),
                    g.headers.clone(),
                    g.stream_timeout,
                )
            };
            let Some(request) = slot.borrow_mut().take() else {
                return new_error(
                    ctx.pos.clone(),
                    "response.begin: response has already been sent",
                );
            };
            let http_version = request.http_version().clone();
            let status = tiny_http::StatusCode(status_code);
            let mut writer = request.into_writer();
            let mut header = format!(
                "HTTP/{} {} {}\r\nContent-Type: {}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n",
                http_version,
                status_code,
                status.default_reason_phrase(),
                content_type
            );
            for (key, value) in headers {
                if key.eq_ignore_ascii_case("content-type")
                    || key.eq_ignore_ascii_case("content-length")
                    || key.eq_ignore_ascii_case("transfer-encoding")
                    || key.eq_ignore_ascii_case("connection")
                {
                    continue;
                }
                header.push_str(&key);
                header.push_str(": ");
                header.push_str(&value);
                header.push_str("\r\n");
            }
            header.push_str("\r\n");
            if let Err(err) = writer
                .write_all(header.as_bytes())
                .and_then(|_| writer.flush())
            {
                return new_error(ctx.pos.clone(), format!("response.begin: {}", err));
            }
            let mut g = s.borrow_mut();
            g.stream_writer = Some(Rc::new(RefCell::new(writer)));
            if let Some(timeout) = timeout {
                g.stream_deadline = Some(Instant::now() + timeout);
            }
            Object::Hash(out.clone())
        }),
    );
    let s = state.clone();
    let out = obj.clone();
    obj.borrow_mut().set(
        "setTimeout",
        native("response.setTimeout", move |ctx, args| {
            let ms = match required_number(ctx, "response.setTimeout", args, 0, "ms") {
                Ok(v) => v.max(0.0) as u64,
                Err(e) => return e,
            };
            let timeout = Duration::from_millis(ms);
            let mut g = s.borrow_mut();
            g.stream_timeout = Some(timeout);
            if g.stream_writer.is_some() && !g.stream_closed {
                g.stream_deadline = Some(Instant::now() + timeout);
            }
            Object::Hash(out.clone())
        }),
    );
    let s = state.clone();
    obj.borrow_mut().set(
        "send",
        native("response.send", move |_ctx, args| {
            let text = match args.first() {
                Some(Object::String(v)) => v.to_string(),
                Some(o) => o.inspect(),
                None => String::new(),
            };
            let mut g = s.borrow_mut();
            if g.content_type.is_none() {
                g.content_type = Some("text/plain".to_string());
            }
            g.body = Some(text.into_bytes());
            Object::Undefined
        }),
    );
    let s = state.clone();
    obj.borrow_mut().set(
        "write",
        native("response.write", move |ctx, args| {
            let text = match args.first() {
                Some(Object::String(v)) => v.to_string(),
                Some(o) => o.inspect(),
                None => String::new(),
            };
            let mut g = s.borrow_mut();
            if g.stream_closed {
                return new_error(ctx.pos.clone(), "response.write: stream closed");
            }
            if g.content_type.is_none() {
                g.content_type = Some("text/plain".to_string());
            }
            // D1.1: In streaming mode, write the chunk immediately to the TCP
            // connection (chunked transfer encoding: "<hex_len>\r\n<data>\r\n").
            if let Some(writer) = g.stream_writer.as_ref() {
                let data = text.as_bytes();
                let mut w = writer.borrow_mut();
                let _ = w.write_all(format!("{:x}\r\n", data.len()).as_bytes());
                let _ = w.write_all(data);
                let _ = w.write_all(b"\r\n");
                let _ = w.flush();
            } else {
                g.body
                    .get_or_insert_with(Vec::new)
                    .extend_from_slice(text.as_bytes());
            }
            Object::Undefined
        }),
    );
    // D1.1: `res.flush()` — in streaming mode, write the terminating chunk
    // (0\r\n\r\n) to close the chunked body. In buffer mode, no-op.
    let s = state.clone();
    obj.borrow_mut().set(
        "flush",
        native("response.flush", move |ctx, _args| {
            if let Err(err) = finish_streaming_response(&s) {
                return new_error(ctx.pos.clone(), format!("response.flush: {}", err));
            }
            Object::Undefined
        }),
    );
    let s = state.clone();
    obj.borrow_mut().set(
        "stream",
        native("response.stream", move |ctx, args| {
            let Some(stream) = args.first().cloned() else {
                return Object::Undefined;
            };
            let mut text = String::new();
            if let Object::Hash(h) = &stream {
                if let Some(read_all) = h.borrow().get("readAll").cloned() {
                    let result = call_script_function(&read_all, ctx.env, &[]);
                    if result.is_runtime_error() {
                        return result;
                    }
                    text = match result {
                        Object::String(v) => v.to_string(),
                        Object::Null | Object::Undefined => String::new(),
                        other => other.inspect(),
                    };
                } else if let Some(read_text) = h.borrow().get("readText").cloned() {
                    loop {
                        let result = call_script_function(&read_text, ctx.env, &[]);
                        if result.is_runtime_error() {
                            return result;
                        }
                        match result {
                            Object::String(v) => text.push_str(&v),
                            Object::Null | Object::Undefined => break,
                            other => {
                                text.push_str(&other.inspect());
                                break;
                            }
                        }
                    }
                } else if let Some(Object::String(v)) = h.borrow().get("text") {
                    text = v.to_string();
                }
            } else {
                text = stream.inspect();
            }
            let mut g = s.borrow_mut();
            if g.content_type.is_none() {
                g.content_type = Some("application/octet-stream".to_string());
            }
            g.body
                .get_or_insert_with(Vec::new)
                .extend_from_slice(text.as_bytes());
            Object::Undefined
        }),
    );
    let s = state.clone();
    obj.borrow_mut().set(
        "json",
        native("response.json", move |_ctx, args| {
            let text = match args.first() {
                Some(Object::Hash(h)) => hash_to_json(&h.borrow()),
                Some(Object::Array(a)) => value_to_json(&Object::Array(a.clone())),
                Some(Object::String(v)) => v.to_string(),
                Some(o) => o.inspect(),
                None => String::new(),
            };
            let mut g = s.borrow_mut();
            g.content_type = Some("application/json".to_string());
            g.body = Some(text.into_bytes());
            Object::Undefined
        }),
    );
    let s = state.clone();
    obj.borrow_mut().set(
        "end",
        native("response.end", move |_ctx, args| {
            if let Some(arg) = args.first() {
                let text = match arg {
                    Object::String(v) => v.to_string(),
                    o => o.inspect(),
                };
                let mut g = s.borrow_mut();
                if g.body.is_none() {
                    g.body = Some(text.into_bytes());
                }
            }
            Object::Undefined
        }),
    );

    Object::Hash(obj)
}

pub(crate) fn finish_streaming_response(
    state: &Rc<RefCell<HttpResponseState>>,
) -> std::io::Result<bool> {
    let mut g = state.borrow_mut();
    if g.stream_closed {
        return Ok(false);
    }
    let Some(writer) = g.stream_writer.take() else {
        return Ok(false);
    };
    {
        let mut w = writer.borrow_mut();
        w.write_all(b"0\r\n\r\n")?;
        w.flush()?;
    }
    g.stream_closed = true;
    g.stream_deadline = None;
    Ok(true)
}

/// Serialize a Hash to a JSON string (minimal, no external dependency).
pub(crate) fn hash_to_json(h: &HashData) -> String {
    let pairs: Vec<String> = h
        .entries
        .iter()
        .map(|(k, v)| format!("{}: {}", json_escape_string(k), value_to_json(v)))
        .collect();
    format!("{{{}}}", pairs.join(", "))
}

pub(crate) fn value_to_json(obj: &Object) -> String {
    match obj {
        Object::Null => "null".to_string(),
        Object::Undefined => "null".to_string(),
        Object::Boolean(b) => b.to_string(),
        Object::Number(n) => format_number(*n),
        Object::String(s) => json_escape_string(s),
        Object::Array(a) => {
            let elems: Vec<String> = a.borrow().elements.iter().map(value_to_json).collect();
            format!("[{}]", elems.join(", "))
        }
        Object::Hash(h) => hash_to_json(&h.borrow()),
        _ => json_escape_string(&obj.inspect()),
    }
}

pub(crate) fn json_escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
