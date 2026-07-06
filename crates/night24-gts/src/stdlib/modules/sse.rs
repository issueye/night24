use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{new_error, str_obj, CallContext, Object};

pub(crate) const SSE_READER_STATE_KEY: &str = "__sse_state__";

pub(crate) fn sse_module() -> Object {
    module(vec![
        ("parse", native("sse.parse", sse_parse)),
        ("reader", native("sse.reader", sse_reader)),
    ])
}

pub(crate) fn sse_parse(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "sse.parse", args);
    let text = match reader.required_string(0, "text") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let events = parse_sse_block(&text.split('\n').collect::<Vec<_>>());
    array(events)
}

pub(crate) fn sse_reader(ctx: &mut CallContext, args: &[Object]) -> Object {
    // Two accepted input shapes, mirroring the Go version:
    //   1. a string of raw SSE text
    //   2. an object carrying `text` / `data`, or a stream with readLine()
    let state = match args.first() {
        Some(Object::String(s)) => {
            let events = parse_sse_block(&s.split('\n').collect::<Vec<_>>());
            SseReaderState::buffered(events)
        }
        Some(Object::Hash(h)) => {
            let hb = h.borrow();
            if let Some(Object::String(s)) = hb.get("text") {
                let events = parse_sse_block(&s.split('\n').collect::<Vec<_>>());
                SseReaderState::buffered(events)
            } else if let Some(value) = hb.get("data") {
                let text = value_to_string(value);
                let events = parse_sse_block(&text.split('\n').collect::<Vec<_>>());
                SseReaderState::buffered(events)
            } else if hb.get("readLine").is_some() {
                SseReaderState::stream(Object::Hash(h.clone()))
            } else {
                return new_error(ctx.pos.clone(), "sse.reader: requires a stream object");
            }
        }
        _ => return new_error(ctx.pos.clone(), "sse.reader requires a stream"),
    };

    let state = Rc::new(RefCell::new(state));

    // Sentinel marker so callers can detect an SSE reader object if needed.
    let instance = ObjectBuilder::new()
        .set(SSE_READER_STATE_KEY, ObjectBuilder::new().build())
        .into_shared();

    let st = state.clone();
    instance.borrow_mut().set(
        "next",
        native("sse.next", move |ctx, _args| sse_reader_next(ctx, &st)),
    );

    let st = state.clone();
    instance.borrow_mut().set(
        "readAll",
        native("sse.readAll", move |ctx, _args| {
            let mut events = Vec::new();
            loop {
                let next = sse_reader_next(ctx, &st);
                if next.is_runtime_error() {
                    return next;
                }
                if matches!(next, Object::Null | Object::Undefined) {
                    break;
                }
                events.push(next);
            }
            array(events)
        }),
    );

    Object::Hash(instance)
}

pub(crate) struct SseReaderState {
    events: Vec<Object>,
    cursor: usize,
    stream: Option<Object>,
    pending: Vec<String>,
    done: bool,
}

impl SseReaderState {
    fn buffered(events: Vec<Object>) -> Self {
        Self {
            events,
            cursor: 0,
            stream: None,
            pending: Vec::new(),
            done: true,
        }
    }

    fn stream(stream: Object) -> Self {
        Self {
            events: Vec::new(),
            cursor: 0,
            stream: Some(stream),
            pending: Vec::new(),
            done: false,
        }
    }
}

pub(crate) fn sse_reader_next(
    ctx: &mut CallContext,
    state: &Rc<RefCell<SseReaderState>>,
) -> Object {
    let mut g = state.borrow_mut();
    if g.stream.is_none() {
        if g.cursor >= g.events.len() {
            return Object::Null;
        }
        let ev = g.events[g.cursor].clone();
        g.cursor += 1;
        return ev;
    }

    loop {
        if g.done {
            return sse_take_pending_event(&mut g);
        }

        let stream = g.stream.clone().unwrap_or(Object::Undefined);
        let read_line = match &stream {
            Object::Hash(h) => h.borrow().get("readLine").cloned(),
            _ => None,
        };
        let read_line = match read_line {
            Some(f) if is_callable(&f) => f,
            _ => return new_error(ctx.pos.clone(), "sse.reader: stream requires readLine()"),
        };
        drop(g);
        let line = call_script_function(&read_line, ctx.env, &[]);
        g = state.borrow_mut();

        match line {
            Object::Null | Object::Undefined => {
                g.done = true;
            }
            Object::String(s) => {
                let line = s.trim_end_matches(['\r', '\n']).to_string();
                if line.is_empty() {
                    let event = sse_take_pending_event(&mut g);
                    if !matches!(event, Object::Null) {
                        return event;
                    }
                } else {
                    g.pending.push(line);
                }
            }
            other if other.is_runtime_error() => return other,
            other => {
                let line = other.inspect();
                if line.is_empty() {
                    let event = sse_take_pending_event(&mut g);
                    if !matches!(event, Object::Null) {
                        return event;
                    }
                } else {
                    g.pending.push(line);
                }
            }
        }
    }
}

fn sse_take_pending_event(state: &mut SseReaderState) -> Object {
    if state.pending.is_empty() {
        return Object::Null;
    }
    let lines = std::mem::take(&mut state.pending);
    let refs: Vec<&str> = lines.iter().map(|line| line.as_str()).collect();
    parse_sse_block(&refs)
        .into_iter()
        .next()
        .unwrap_or(Object::Null)
}

pub(crate) fn parse_sse_block(lines: &[&str]) -> Vec<Object> {
    let mut blocks: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for raw in lines {
        let line = raw.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(line.to_string());
    }
    if !current.is_empty() {
        blocks.push(current);
    }

    let mut events = Vec::with_capacity(blocks.len());
    for block in blocks {
        let mut event_type = "message".to_string();
        let mut event_id = String::new();
        let mut retry = String::new();
        let mut data_parts: Vec<String> = Vec::new();
        for line in block {
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = match line.find(':') {
                Some(idx) => {
                    let f = line[..idx].to_string();
                    let mut v = line[idx + 1..].to_string();
                    if let Some(stripped) = v.strip_prefix(' ') {
                        v = stripped.to_string();
                    }
                    (f, v)
                }
                None => (line.clone(), String::new()),
            };
            match field.as_str() {
                "event" => event_type = value,
                "data" => data_parts.push(value),
                "id" => event_id = value,
                "retry" => retry = value,
                _ => {}
            }
        }
        let mut event = ObjectBuilder::new()
            .set("type", str_obj(event_type.clone()))
            .set("event", str_obj(event_type.clone()))
            .set("data", str_obj(data_parts.join("\n")));
        if !event_id.is_empty() {
            event.insert("id", str_obj(event_id));
        }
        if !retry.is_empty() {
            event.insert("retry", str_obj(retry));
        }
        events.push(event.build());
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object_field<'a>(object: &'a Object, key: &str) -> Object {
        let Object::Hash(hash) = object else {
            panic!("expected event object");
        };
        hash.borrow()
            .get(key)
            .cloned()
            .unwrap_or_else(|| panic!("expected field {key}"))
    }

    fn string_field(object: &Object, key: &str) -> String {
        match object_field(object, key) {
            Object::String(value) => value.to_string(),
            _ => panic!("expected string field {key}"),
        }
    }

    #[test]
    fn parse_sse_block_reads_event_data_id_and_retry() {
        let events = parse_sse_block(&[
            "event: update",
            "data: first",
            "data: second",
            "id: 42",
            "retry: 1500",
            "",
        ]);

        assert_eq!(events.len(), 1);
        assert_eq!(string_field(&events[0], "type"), "update");
        assert_eq!(string_field(&events[0], "event"), "update");
        assert_eq!(string_field(&events[0], "data"), "first\nsecond");
        assert_eq!(string_field(&events[0], "id"), "42");
        assert_eq!(string_field(&events[0], "retry"), "1500");
    }
}

// ---------------------------------------------------------------------------
// db: SQLite-backed database module (@std/db)
// ---------------------------------------------------------------------------
