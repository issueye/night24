//! @std/gtp/server - GTP server module (F2.1, Phase 3)
//!
//! Provides a GTP server that listens for incoming `call` frames and dispatches
//! them to a script-side handler, returning its result as a `result` frame.
//!
//! The VM is synchronous and single-threaded, so the server is inline (like
//! `@std/net/socket/server`): `listen(addr)` binds a TCP listener, and
//! `acceptOne()`/`accept()` drain pending connections synchronously. Each
//! accepted connection reads one `call` frame, invokes the handler, and writes
//! back one `result` frame.
//!
//! Wire format is GTP (JSON Lines over tcp), symmetric with `@std/gtp/client`.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::rc::Rc;

use crate::gtp::frame::{Frame, GtpError, Value};
use crate::object::{bool_obj, new_error, num_obj, str_obj, CallContext, HashData, Object};
use crate::stdlib::helpers::native;

/// Create the @std/gtp/server module.
pub fn gtp_server_module() -> Object {
    let mut exports = HashData::default();
    exports.set(
        "createServer",
        native("gtp.createServer", gtp_create_server),
    );
    exports.set("listen", native("gtp.listen", gtp_listen_standalone));
    Object::Hash(Rc::new(RefCell::new(exports)))
}

/// `gtp.createServer(handler)` → server object.
///
/// `handler` is a script function `(frameObj) => resultValue` (or throws to
/// signal an error). The server invokes it once per accepted `call` frame.
fn gtp_create_server(ctx: &mut CallContext, args: &[Object]) -> Object {
    let handler = match args.first() {
        Some(h @ (Object::Function(_) | Object::Builtin(_) | Object::Closure(_))) => h.clone(),
        _ => {
            return new_error(
                ctx.pos.clone(),
                "gtp.createServer: handler must be a function",
            )
        }
    };
    let state: Rc<RefCell<GtpServerState>> = Rc::new(RefCell::new(GtpServerState {
        handler,
        listener: None,
    }));
    let obj = Rc::new(RefCell::new(HashData::default()));

    let s = state.clone();
    obj.borrow_mut().set(
        "listen",
        native("gtp.server.listen", move |ctx, args| {
            gtp_server_listen(ctx, &s, args)
        }),
    );
    let s = state.clone();
    obj.borrow_mut().set(
        "acceptOne",
        native("gtp.server.acceptOne", move |ctx, _args| {
            gtp_accept_one(ctx, &s)
        }),
    );
    let s = state.clone();
    obj.borrow_mut().set(
        "accept",
        native("gtp.server.accept", move |ctx, _args| gtp_accept(ctx, &s)),
    );
    let s = state.clone();
    obj.borrow_mut().set(
        "close",
        native("gtp.server.close", move |_ctx, _args| {
            s.borrow_mut().listener = None;
            Object::Undefined
        }),
    );
    let s = state.clone();
    obj.borrow_mut().set(
        "isAlive",
        native("gtp.server.isAlive", move |_ctx, _args| {
            bool_obj(s.borrow().listener.is_some())
        }),
    );
    Object::Hash(obj)
}

struct GtpServerState {
    handler: Object,
    listener: Option<TcpListener>,
}

/// Standalone `gtp.listen` is a no-op pointer: callers use `server.listen`.
fn gtp_listen_standalone(ctx: &mut CallContext, _args: &[Object]) -> Object {
    new_error(
        ctx.pos.clone(),
        "gtp.listen: use server.listen(addr) on a created server",
    )
}

/// `server.listen(addr)` — bind "host:port" and return an `{host, port}` info.
fn gtp_server_listen(
    ctx: &mut CallContext,
    state: &Rc<RefCell<GtpServerState>>,
    args: &[Object],
) -> Object {
    let addr = match args.first() {
        Some(Object::String(s)) => s.as_ref().clone(),
        _ => {
            return new_error(
                ctx.pos.clone(),
                "gtp.server.listen: expected address string",
            )
        }
    };
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            return new_error(
                ctx.pos.clone(),
                format!("gtp.server.listen: bind {} failed: {}", addr, e),
            )
        }
    };
    let _ = listener.set_nonblocking(true);
    let bound = listener
        .local_addr()
        .map(|a| (a.ip().to_string(), a.port()))
        .unwrap_or_else(|_| ("0.0.0.0".to_string(), 0));
    state.borrow_mut().listener = Some(listener);
    let info = Rc::new(RefCell::new(HashData::default()));
    info.borrow_mut().set("host", str_obj(bound.0));
    info.borrow_mut().set("port", num_obj(bound.1 as f64));
    Object::Hash(info)
}

/// `server.acceptOne()` — accept one pending connection (non-blocking; returns
/// an error if none pending), handle its call frame, send the result.
fn gtp_accept_one(ctx: &mut CallContext, state: &Rc<RefCell<GtpServerState>>) -> Object {
    let listener = {
        let guard = state.borrow();
        match guard.listener.as_ref() {
            Some(l) => l.try_clone(),
            None => {
                return new_error(
                    ctx.pos.clone(),
                    "gtp.server.acceptOne: server is not listening",
                )
            }
        }
    };
    let listener = match listener {
        Ok(l) => l,
        Err(e) => return new_error(ctx.pos.clone(), format!("gtp.server.acceptOne: {}", e)),
    };
    let (stream, _peer) = match listener.accept() {
        Ok(pair) => pair,
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            return new_error(
                ctx.pos.clone(),
                "gtp.server.acceptOne: no pending connection (WouldBlock)",
            )
        }
        Err(e) => {
            return new_error(
                ctx.pos.clone(),
                format!("gtp.server.acceptOne: accept failed: {}", e),
            )
        }
    };
    handle_one_connection(ctx, state, stream)
}

/// `server.accept()` — drain all currently-pending connections; return count.
fn gtp_accept(ctx: &mut CallContext, state: &Rc<RefCell<GtpServerState>>) -> Object {
    let listener = {
        let guard = state.borrow();
        match guard.listener.as_ref() {
            Some(l) => l.try_clone(),
            None => {
                return new_error(
                    ctx.pos.clone(),
                    "gtp.server.accept: server is not listening",
                )
            }
        }
    };
    let listener = match listener {
        Ok(l) => l,
        Err(e) => return new_error(ctx.pos.clone(), format!("gtp.server.accept: {}", e)),
    };
    let mut handled = 0i64;
    loop {
        match listener.accept() {
            Ok((stream, _peer)) => {
                let r = handle_one_connection(ctx, state, stream);
                if r.is_runtime_error() {
                    return r;
                }
                handled += 1;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => {
                return new_error(
                    ctx.pos.clone(),
                    format!("gtp.server.accept: accept failed: {}", e),
                )
            }
        }
    }
    num_obj(handled as f64)
}

/// Read one call frame from the stream, invoke the handler, write the result.
fn handle_one_connection(
    ctx: &mut CallContext,
    state: &Rc<RefCell<GtpServerState>>,
    stream: std::net::TcpStream,
) -> Object {
    let cloned = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => return new_error(ctx.pos.clone(), format!("gtp.server: clone stream: {}", e)),
    };
    let mut reader = BufReader::new(cloned);
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => {
            return new_error(
                ctx.pos.clone(),
                "gtp.server: client closed before sending a frame",
            )
        }
        Ok(_) => {}
        Err(e) => return new_error(ctx.pos.clone(), format!("gtp.server: read failed: {}", e)),
    }
    let frame: Frame = match serde_json::from_str(line.trim()) {
        Ok(f) => f,
        Err(e) => {
            return new_error(
                ctx.pos.clone(),
                format!("gtp.server: invalid frame from client: {}", e),
            )
        }
    };
    let frame_obj = frame_to_object(&frame);
    let handler = state.borrow().handler.clone();
    let result = crate::stdlib::helpers::call_script_function(&handler, ctx.env, &[frame_obj]);
    let response = match &result {
        Object::Error(err) => {
            let msg = err.borrow().message.clone();
            Frame::error_result(
                frame.id.clone(),
                GtpError {
                    name: "HandlerError".to_string(),
                    message: msg,
                    code: None,
                    details: None,
                },
            )
        }
        _ => Frame::ok_result(frame.id.clone(), object_to_gtp_value(&result)),
    };
    let mut out = serde_json::to_vec(&response).unwrap_or_default();
    out.push(b'\n');
    let mut stream = stream;
    let _ = stream.write_all(&out);
    let _ = stream.flush();
    Object::Undefined
}

/// Convert a GTP Frame into a script-side object (module/method/args/id/type).
fn frame_to_object(frame: &Frame) -> Object {
    let obj = Rc::new(RefCell::new(HashData::default()));
    obj.borrow_mut().set("id", str_obj(frame.id.clone()));
    obj.borrow_mut()
        .set("type", str_obj(frame.frame_type.clone()));
    if let Some(m) = &frame.module {
        obj.borrow_mut().set("module", str_obj(m.clone()));
    }
    if let Some(m) = &frame.method {
        obj.borrow_mut().set("method", str_obj(m.clone()));
    }
    if let Some(args) = &frame.args {
        let arr: Vec<Object> = args.iter().map(gtp_value_to_object).collect();
        obj.borrow_mut()
            .set("args", crate::stdlib::helpers::array(arr));
    }
    Object::Hash(obj)
}

/// Convert a script Object into a GTP Value for the response.
fn object_to_gtp_value(obj: &Object) -> Value {
    match obj {
        Object::Undefined => Value::undefined(),
        Object::Null => Value::null(),
        Object::Boolean(b) => Value::boolean(*b),
        Object::Number(n) => Value::number(*n),
        Object::String(s) => Value::string(s.as_ref().clone()),
        Object::Array(arr) => {
            let items: Vec<Value> = arr
                .borrow()
                .elements
                .iter()
                .map(object_to_gtp_value)
                .collect();
            Value::array(items)
        }
        _ => Value::string(format!("<{}>", obj.type_tag())),
    }
}

/// Convert a GTP Value into a script Object (mirrors the client's converter).
fn gtp_value_to_object(val: &Value) -> Object {
    match val.value_type.as_str() {
        "undefined" => Object::Undefined,
        "null" => Object::Null,
        "boolean" => {
            if let Some(serde_json::Value::Bool(b)) = &val.v {
                bool_obj(*b)
            } else {
                Object::Undefined
            }
        }
        "number" => {
            if let Some(serde_json::Value::Number(n)) = &val.v {
                num_obj(n.as_f64().unwrap_or(0.0))
            } else {
                Object::Undefined
            }
        }
        "string" => {
            if let Some(serde_json::Value::String(s)) = &val.v {
                str_obj(s.clone())
            } else {
                Object::Undefined
            }
        }
        _ => Object::Undefined,
    }
}
