use std::rc::Rc;

use super::super::helpers::*;
use super::net_ws_client::{compute_accept_key, find_subsequence, new_ws_conn_object, WsConn};
use crate::object::{new_error, num_obj, str_obj, CallContext, Object};

pub(crate) fn ws_server_module() -> Object {
    module(vec![
        (
            "createServer",
            native("ws.createServer", ws_server_create_server),
        ),
        ("upgrade", native("ws.upgrade", ws_server_upgrade)),
    ])
}

/// Background-free WS server: binds a TCP listener (non-blocking) and exposes
/// `accept(handler)`/`acceptOne(handler)`/`close`. Each accept performs the
/// WS handshake inline then invokes the handler with the upgraded connection.
/// `upgrade(reqObj)` is a no-op stub here because the synchronous VM has no
/// HTTP request abstraction to hijack; it returns an error explaining this.
const WS_SERVER_STATE_KEY: &str = "__ws_server__";

pub(crate) struct WsServer {
    listener: std::cell::RefCell<Option<std::net::TcpListener>>,
}

pub(crate) fn ws_server_create_server(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "ws.createServer", args);
    let port = match reader.required_number(0, "port") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let handler = match args.get(1) {
        Some(value) if is_callable(value) => Some(value.clone()),
        _ => None,
    };
    let addr = format!("0.0.0.0:{}", port as i64);
    let listener = match std::net::TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => return new_error(ctx.pos.clone(), format!("ws.createServer: {}", e)),
    };
    let _ = listener.set_nonblocking(true);
    let bound_port = listener
        .local_addr()
        .map(|a| a.port())
        .unwrap_or(port as u16);

    let server = Rc::new(WsServer {
        listener: std::cell::RefCell::new(Some(listener)),
    });
    let obj = ObjectBuilder::new()
        .set(
            WS_SERVER_STATE_KEY,
            ObjectBuilder::new()
                .set("__ws_handler__", handler.unwrap_or(Object::Undefined))
                .build(),
        )
        .set("port", num_obj(bound_port as f64))
        .set("address", str_obj(format!(":{}", bound_port)))
        .into_shared();

    let s = server.clone();
    obj.borrow_mut().set(
        "acceptOne",
        native("ws.acceptOne", move |ctx, args| {
            ws_accept_one(ctx, &s, args)
        }),
    );
    let s = server.clone();
    obj.borrow_mut().set(
        "accept",
        native("ws.accept", move |ctx, args| ws_accept_one(ctx, &s, args)),
    );
    let s = server.clone();
    obj.borrow_mut().set(
        "close",
        native("ws.serverClose", move |_ctx, _args| {
            let mut guard = s.listener.borrow_mut();
            *guard = None;
            Object::Undefined
        }),
    );

    Object::Hash(obj)
}

pub(crate) fn ws_accept_one(
    ctx: &mut CallContext,
    server: &Rc<WsServer>,
    args: &[Object],
) -> Object {
    let handler = match args.first() {
        Some(value) if is_callable(value) => Some(value.clone()),
        _ => None,
    };
    let guard = server.listener.borrow();
    let listener = match guard.as_ref() {
        Some(l) => l,
        None => return new_error(ctx.pos.clone(), "ws.acceptOne: server closed"),
    };
    let (mut stream, _addr) = match listener.accept() {
        Ok(pair) => pair,
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            return new_error(
                ctx.pos.clone(),
                "ws.acceptOne: no pending connection (WouldBlock)",
            )
        }
        Err(e) => return new_error(ctx.pos.clone(), format!("ws.acceptOne: {}", e)),
    };
    drop(guard);
    // Reset to blocking for the synchronous handshake + read/write loop.
    let _ = stream.set_nonblocking(false);

    // Perform the server-side WS handshake.
    match ws_server_handshake(&mut stream) {
        Ok(()) => {}
        Err(e) => return new_error(ctx.pos.clone(), format!("ws.acceptOne: handshake: {}", e)),
    }
    let conn = new_ws_conn_object(Rc::new(WsConn {
        stream: std::cell::RefCell::new(Some(Box::new(stream))),
    }));

    match handler {
        Some(h) => call_script_function(&h, ctx.env, &[conn]),
        None => conn,
    }
}

/// Read the client's HTTP upgrade request, validate it, and write back the
/// "101 Switching Protocols" response with the computed accept key.
fn ws_server_handshake(stream: &mut std::net::TcpStream) -> std::io::Result<()> {
    use std::io::{Read, Write};
    let mut collected: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "client closed before handshake",
            ));
        }
        collected.extend_from_slice(&buf[..n]);
        if let Some(idx) = find_subsequence(&collected, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&collected[..idx]).to_string();
            let key = extract_header(&head, "Sec-WebSocket-Key")
                .ok_or_else(|| std::io::Error::other("missing Sec-WebSocket-Key"))?;
            let accept = compute_accept_key(&key);
            let resp = format!(
                "HTTP/1.1 101 Switching Protocols\r\n\
                 Upgrade: websocket\r\n\
                 Connection: Upgrade\r\n\
                 Sec-WebSocket-Accept: {}\r\n\
                 \r\n",
                accept
            );
            stream.write_all(resp.as_bytes())?;
            stream.flush()?;
            return Ok(());
        }
        if collected.len() > 64 * 1024 {
            return Err(std::io::Error::other("handshake request too large"));
        }
    }
}

pub(crate) fn extract_header(head: &str, name: &str) -> Option<String> {
    for line in head.lines() {
        if let Some(idx) = line.find(':') {
            let key = line[..idx].trim();
            if key.eq_ignore_ascii_case(name) {
                return Some(line[idx + 1..].trim().to_string());
            }
        }
    }
    None
}

pub(crate) fn ws_server_upgrade(ctx: &mut CallContext, _args: &[Object]) -> Object {
    // The synchronous VM has no live HTTP request/response pair to hijack.
    // Scripts that want a WS server should use ws.createServer(port, handler)
    // + acceptOne, which performs the handshake inline.
    new_error(
        ctx.pos.clone(),
        "ws.upgrade is not supported in the synchronous runtime; use ws.createServer(port, handler).acceptOne() instead",
    )
}

// ---------------------------------------------------------------------------
// net/http/server: synchronous HTTP server backed by tiny_http
// (@std/net/http/server)
// ---------------------------------------------------------------------------
