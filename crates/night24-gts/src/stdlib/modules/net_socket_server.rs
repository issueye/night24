use std::rc::Rc;

use super::super::helpers::*;
use super::net_socket_client::new_socket_conn_object;
use crate::object::{new_error, num_obj, str_obj, CallContext, Object};

/// The synchronous VM has no event loop, so a Go-style background accept loop
/// cannot be reproduced. We expose the same `listen` / `createServer` surface
/// but the server runs in-line: each call to `acceptOne(handler)` blocks for a
/// single connection and invokes the handler synchronously. `listen(port,
/// handler)` returns the server object without spawning, and exposes
/// `acceptOne`/`close` for explicit control.
const SOCKET_SERVER_STATE_KEY: &str = "__socket_server__";

pub(crate) struct SocketServer {
    listener: std::cell::RefCell<Option<std::net::TcpListener>>,
    /// Handler registered at `listen` time, used by `acceptOne` when the
    /// caller does not pass one explicitly.
    handler: std::cell::RefCell<Option<Object>>,
}

pub(crate) fn socket_server_module() -> Object {
    module(vec![
        ("listen", native("socket.listen", socket_listen)),
        ("createServer", native("socket.createServer", socket_listen)),
    ])
}

pub(crate) fn socket_listen(ctx: &mut CallContext, args: &[Object]) -> Object {
    let reader = ArgReader::new(ctx, "socket.listen", args);
    let port = match reader.required_number(0, "port") {
        Ok(v) => v,
        Err(e) => return e,
    };
    // Capture the handler (if provided) so acceptOne can use it without
    // re-passing it on every call.
    let handler = match args.get(1) {
        Some(value) if is_callable(value) => Some(value.clone()),
        _ => None,
    };
    let addr = format!("0.0.0.0:{}", port as i64);
    let listener = match std::net::TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => return new_error(ctx.pos.clone(), format!("socket.listen: {}", e)),
    };
    // Don't block the whole VM on accept; set non-blocking so acceptOne can
    // be polled explicitly.
    let _ = listener.set_nonblocking(true);
    let bound_port = listener
        .local_addr()
        .map(|a| a.port())
        .unwrap_or(port as u16);

    let server = Rc::new(SocketServer {
        listener: std::cell::RefCell::new(Some(listener)),
        handler: std::cell::RefCell::new(handler),
    });
    let obj = ObjectBuilder::new()
        .set(SOCKET_SERVER_STATE_KEY, ObjectBuilder::new().build())
        .set("port", num_obj(bound_port as f64))
        .set("address", str_obj(format!(":{}", bound_port)))
        .into_shared();

    let s = server.clone();
    obj.borrow_mut().set(
        "acceptOne",
        native("server.acceptOne", move |ctx, args| {
            socket_accept_one(ctx, &s, args)
        }),
    );
    let s = server.clone();
    obj.borrow_mut().set(
        "accept",
        native("server.accept", move |ctx, args| {
            socket_accept_one(ctx, &s, args)
        }),
    );
    let s = server.clone();
    obj.borrow_mut().set(
        "close",
        native("server.close", move |_ctx, _args| {
            let mut guard = s.listener.borrow_mut();
            *guard = None; // drop listener
            Object::Undefined
        }),
    );

    Object::Hash(obj)
}

pub(crate) fn socket_accept_one(
    ctx: &mut CallContext,
    server: &Rc<SocketServer>,
    args: &[Object],
) -> Object {
    // Prefer an explicitly-passed handler; fall back to the one registered at
    // listen time.
    let handler = match args.first() {
        Some(value) if is_callable(value) => Some(value.clone()),
        Some(_) => {
            return new_error(
                ctx.pos.clone(),
                "server.acceptOne: handler must be a function",
            )
        }
        None => server.handler.borrow().clone(),
    };
    let handler = match handler {
        Some(h) => h,
        None => {
            return new_error(
                ctx.pos.clone(),
                "server.acceptOne requires a handler function",
            )
        }
    };

    let guard = server.listener.borrow();
    let listener = match guard.as_ref() {
        Some(l) => l,
        None => return new_error(ctx.pos.clone(), "server.acceptOne: server closed"),
    };
    // The listener is non-blocking; return a sentinel if no pending connection.
    let (stream, _addr) = match listener.accept() {
        Ok(pair) => pair,
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            return new_error(
                ctx.pos.clone(),
                "server.acceptOne: no pending connection (WouldBlock)",
            )
        }
        Err(e) => return new_error(ctx.pos.clone(), format!("server.acceptOne: {}", e)),
    };
    // Reset the accepted stream to blocking for synchronous read/write.
    let _ = stream.set_nonblocking(false);
    let remote = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();
    let local = stream
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();
    let conn_obj = new_socket_conn_object(stream, remote, local);
    // drop the listener borrow before invoking the handler, in case the
    // handler triggers another borrow (e.g. close).
    drop(guard);
    call_script_function(&handler, ctx.env, &[conn_obj])
}

// ---------------------------------------------------------------------------
// runtime: spawn an isolated sub-script (@std/runtime)
// ---------------------------------------------------------------------------
