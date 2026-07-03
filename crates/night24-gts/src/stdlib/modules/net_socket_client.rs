use std::cell::RefCell;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{new_error, num_obj, str_obj, CallContext, HashData, Object};

/// A live TCP stream held inside a Hash via a sentinel state cell. The GTS VM
/// is single-threaded (synchronous tree-walker), so a plain `RefCell` is safe.
pub(crate) struct SocketStream {
    stream: std::cell::RefCell<Option<std::net::TcpStream>>,
}

pub(crate) const SOCKET_CONN_STATE_KEY: &str = "__socket_conn__";

pub(crate) fn new_socket_conn_object(
    stream: std::net::TcpStream,
    remote: String,
    local: String,
) -> Object {
    let conn = Rc::new(SocketStream {
        stream: std::cell::RefCell::new(Some(stream)),
    });
    let obj = Rc::new(RefCell::new(HashData::default()));
    obj.borrow_mut().set(
        SOCKET_CONN_STATE_KEY,
        Object::Hash(Rc::new(RefCell::new(HashData::default()))),
    );
    obj.borrow_mut().set("remoteAddr", str_obj(remote));
    obj.borrow_mut().set("localAddr", str_obj(local));

    let c = conn.clone();
    obj.borrow_mut().set(
        "write",
        native("socket.write", move |ctx, args| socket_write(ctx, &c, args)),
    );
    let c = conn.clone();
    obj.borrow_mut().set(
        "send",
        native("socket.send", move |ctx, args| socket_write(ctx, &c, args)),
    );
    let c = conn.clone();
    obj.borrow_mut().set(
        "read",
        native("socket.read", move |ctx, args| socket_read(ctx, &c, args)),
    );
    let c = conn.clone();
    obj.borrow_mut().set(
        "recv",
        native("socket.recv", move |ctx, args| socket_recv(ctx, &c, args)),
    );
    let c = conn.clone();
    obj.borrow_mut().set(
        "close",
        native("socket.close", move |_ctx, _args| socket_close(&c)),
    );
    let c = conn.clone();
    obj.borrow_mut().set(
        "setDeadline",
        native("socket.setDeadline", move |ctx, args| {
            socket_set_deadline(ctx, &c, args)
        }),
    );

    Object::Hash(obj)
}

pub(crate) fn socket_client_module() -> Object {
    module(vec![
        ("connect", native("socket.connect", socket_connect)),
        ("dial", native("socket.connect", socket_connect)),
    ])
}

pub(crate) fn socket_connect(ctx: &mut CallContext, args: &[Object]) -> Object {
    let host = match required_string(ctx, "socket.connect", args, 0, "host") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let port = match required_number(ctx, "socket.connect", args, 1, "port") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let addr = format!("{}:{}", host, port as i64);
    let socket_addr = match resolve_socket_addr(&host, port as u16) {
        Ok(sa) => sa,
        Err(e) => return new_error(ctx.pos.clone(), format!("socket.connect: {}", e)),
    };
    let timeout = std::time::Duration::from_secs(30);
    let stream = match std::net::TcpStream::connect_timeout(&socket_addr, timeout) {
        Ok(s) => s,
        Err(e) => return new_error(ctx.pos.clone(), format!("socket.connect: {} ({})", e, addr)),
    };
    let remote = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();
    let local = stream
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();
    new_socket_conn_object(stream, remote, local)
}

pub(crate) fn socket_write(
    ctx: &mut CallContext,
    conn: &Rc<SocketStream>,
    args: &[Object],
) -> Object {
    let data = match args.first() {
        Some(v) => v.inspect().into_bytes(),
        None => return new_error(ctx.pos.clone(), "socket.write requires data"),
    };
    let mut guard = conn.stream.borrow_mut();
    let stream = match guard.as_mut() {
        Some(s) => s,
        None => return new_error(ctx.pos.clone(), "socket.write: connection closed"),
    };
    use std::io::Write;
    match stream.write_all(&data).and_then(|_| stream.flush()) {
        Ok(_) => num_obj(data.len() as f64),
        Err(e) => new_error(ctx.pos.clone(), format!("socket.write: {}", e)),
    }
}

pub(crate) fn socket_read(
    ctx: &mut CallContext,
    conn: &Rc<SocketStream>,
    args: &[Object],
) -> Object {
    let buf_size = match args.first() {
        Some(Object::Number(n)) => (*n as usize).max(1),
        _ => 4096,
    };
    socket_read_impl(ctx, conn, buf_size, "socket.read")
}

pub(crate) fn socket_recv(
    ctx: &mut CallContext,
    conn: &Rc<SocketStream>,
    _args: &[Object],
) -> Object {
    socket_read_impl(ctx, conn, 4096, "socket.recv")
}

pub(crate) fn socket_read_impl(
    ctx: &mut CallContext,
    conn: &Rc<SocketStream>,
    buf_size: usize,
    name: &str,
) -> Object {
    use std::io::Read;
    let mut guard = conn.stream.borrow_mut();
    let stream = match guard.as_mut() {
        Some(s) => s,
        None => return new_error(ctx.pos.clone(), format!("{}: connection closed", name)),
    };
    let mut buf = vec![0u8; buf_size];
    match stream.read(&mut buf) {
        Ok(0) => Object::Null, // EOF
        Ok(n) => str_obj(String::from_utf8_lossy(&buf[..n]).into_owned()),
        Err(e) => new_error(ctx.pos.clone(), format!("{}: {}", name, e)),
    }
}

pub(crate) fn socket_close(conn: &Rc<SocketStream>) -> Object {
    let mut guard = conn.stream.borrow_mut();
    *guard = None; // Drop the TcpStream, closing the underlying socket.
    Object::Undefined
}

pub(crate) fn socket_set_deadline(
    ctx: &mut CallContext,
    conn: &Rc<SocketStream>,
    args: &[Object],
) -> Object {
    let ms = match required_number(ctx, "socket.setDeadline", args, 0, "timeout") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let guard = conn.stream.borrow();
    match guard.as_ref() {
        Some(stream) => {
            let dur = Some(std::time::Duration::from_millis(ms.max(0.0) as u64));
            // Set both read and write timeouts; ignore errors (e.g. unsupported).
            let _ = stream.set_read_timeout(dur);
            let _ = stream.set_write_timeout(dur);
            Object::Undefined
        }
        None => new_error(ctx.pos.clone(), "socket.setDeadline: connection closed"),
    }
}

// ---------------------------------------------------------------------------
// net/socket/server: synchronous TCP server (@std/net/socket/server)
// ---------------------------------------------------------------------------
