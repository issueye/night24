use std::cell::RefCell;
use std::io::{Read, Write};
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{new_error, str_obj, CallContext, HashData, Object};

pub(crate) const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

pub(crate) const WS_OP_CONTINUATION: u8 = 0;

pub(crate) const WS_OP_TEXT: u8 = 1;

pub(crate) const WS_OP_BINARY: u8 = 2;

pub(crate) const WS_OP_CLOSE: u8 = 8;

pub(crate) const WS_OP_PING: u8 = 9;

pub(crate) const WS_OP_PONG: u8 = 10;

/// A live WebSocket connection wrapping a blocking `TcpStream`. The frame
/// reader/writer mirror Go's `wsConn` exactly (RFC 6455 framing).
pub(crate) trait WsStream: Read + Write {}

impl<T: Read + Write> WsStream for T {}

pub(crate) struct WsConn {
    pub(crate) stream: std::cell::RefCell<Option<Box<dyn WsStream>>>,
}

pub(crate) const WS_CONN_STATE_KEY: &str = "__ws_conn__";

pub(crate) fn ws_client_module() -> Object {
    module(vec![("connect", native("ws.connect", ws_client_connect))])
}

pub(crate) fn ws_client_connect(ctx: &mut CallContext, args: &[Object]) -> Object {
    let url = match required_string(ctx, "ws.connect", args, 0, "url") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut headers: Vec<(String, String)> = Vec::new();
    if let Some(Object::Hash(h)) = args.get(1) {
        for (k, v) in &h.borrow().entries {
            headers.push((k.clone(), v.inspect()));
        }
    }
    let conn = match dial_websocket(&url, &headers) {
        Ok(c) => c,
        Err(e) => return new_error(ctx.pos.clone(), format!("ws.connect: {}", e)),
    };
    new_ws_conn_object(Rc::new(WsConn {
        stream: std::cell::RefCell::new(Some(Box::new(conn))),
    }))
}

/// Perform the WebSocket opening handshake over a fresh TCP connection and
/// return the upgraded stream. Mirrors Go's `dialWebSocket`.
fn dial_websocket(url: &str, headers: &[(String, String)]) -> std::io::Result<std::net::TcpStream> {
    let is_secure = url.starts_with("wss://");
    let stripped = url
        .strip_prefix("ws://")
        .or_else(|| url.strip_prefix("wss://"))
        .unwrap_or(url);
    let (host, path) = match stripped.find('/') {
        Some(i) => (&stripped[..i], &stripped[i..]),
        None => (stripped, "/"),
    };
    let host_port = if host.contains(':') {
        host.to_string()
    } else if is_secure {
        format!("{}:443", host)
    } else {
        format!("{}:80", host)
    };

    let socket_addr = resolve_socket_addr(
        host_port.split(':').next().unwrap_or(host),
        host_port
            .rsplit(':')
            .next()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(if is_secure { 443 } else { 80 }),
    )?;
    let mut stream =
        std::net::TcpStream::connect_timeout(&socket_addr, std::time::Duration::from_secs(10))?;

    // Generate the client nonce (16 random bytes, base64-encoded).
    let mut nonce_bytes = [0u8; 16];
    if !getrandom_inner(&mut nonce_bytes) {
        return Err(std::io::Error::other("random source unavailable"));
    }
    let nonce = base64_std_encode(&nonce_bytes);

    let mut req = String::new();
    req.push_str(&format!("GET {} HTTP/1.1\r\n", path));
    req.push_str(&format!("Host: {}\r\n", host));
    req.push_str("Upgrade: websocket\r\n");
    req.push_str("Connection: Upgrade\r\n");
    req.push_str(&format!("Sec-WebSocket-Key: {}\r\n", nonce));
    req.push_str("Sec-WebSocket-Version: 13\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    req.push_str("\r\n");
    use std::io::Write;
    stream.write_all(req.as_bytes())?;
    stream.flush()?;

    // Read the HTTP response, looking for "101 Switching Protocols" and the
    // Sec-WebSocket-Accept header.
    use std::io::Read;
    let mut buf = [0u8; 4096];
    let mut collected = Vec::new();
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed during handshake",
            ));
        }
        collected.extend_from_slice(&buf[..n]);
        // The header section ends at "\r\n\r\n".
        if let Some(idx) = find_subsequence(&collected, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&collected[..idx]).to_string();
            if !head.contains(" 101 ") {
                return Err(std::io::Error::other("unexpected handshake status"));
            }
            let expected = compute_accept_key(&nonce);
            if !head.contains(&format!("Sec-WebSocket-Accept: {}", expected)) {
                return Err(std::io::Error::other("invalid Sec-WebSocket-Accept"));
            }
            return Ok(stream);
        }
        if collected.len() > 64 * 1024 {
            return Err(std::io::Error::other("handshake response too large"));
        }
    }
}

pub(crate) fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// RFC 6455 accept-key: base64(sha1(client_key + GUID)).
pub(crate) fn compute_accept_key(key: &str) -> String {
    let digest = sha1(format!("{}{}", key, WS_GUID).as_bytes());
    base64_std_encode(&digest)
}

// --- Frame read/write (RFC 6455 §5) ---------------------------------------

pub(crate) fn ws_write_frame(
    stream: &mut dyn WsStream,
    opcode: u8,
    payload: &[u8],
) -> std::io::Result<()> {
    let mut frame = Vec::with_capacity(2 + payload.len() + 8);
    frame.push(0x80 | opcode); // FIN=1, opcode
    let len = payload.len();
    if len <= 125 {
        frame.push(len as u8);
    } else if len <= 65535 {
        frame.push(126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    frame.extend_from_slice(payload);
    stream.write_all(&frame)?;
    stream.flush()
}

pub(crate) fn ws_read_frame(stream: &mut dyn WsStream) -> std::io::Result<(u8, Vec<u8>)> {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header)?;
    let fin = (header[0] & 0x80) != 0;
    let opcode = header[0] & 0x0F;
    let masked = (header[1] & 0x80) != 0;
    let mut length = (header[1] & 0x7F) as u64;
    if length == 126 {
        let mut ext = [0u8; 2];
        stream.read_exact(&mut ext)?;
        length = u16::from_be_bytes(ext) as u64;
    } else if length == 127 {
        let mut ext = [0u8; 8];
        stream.read_exact(&mut ext)?;
        length = u64::from_be_bytes(ext);
    }
    let mut mask_key = [0u8; 4];
    if masked {
        stream.read_exact(&mut mask_key)?;
    }
    let mut payload = vec![0u8; length as usize];
    stream.read_exact(&mut payload)?;
    if masked {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask_key[i % 4];
        }
    }
    if fin {
        Ok((opcode, payload))
    } else {
        // Fragmented: keep reading until a FIN frame arrives (concatenate).
        let (next_op, rest) = ws_read_frame(stream)?;
        let _ = next_op;
        payload.extend_from_slice(&rest);
        Ok((opcode, payload))
    }
}

/// Read the next data message (text/binary), automatically answering Pings
/// with Pongs and surfacing Close as EOF. Mirrors Go's `ReadMessage`.
fn ws_read_message(stream: &mut dyn WsStream) -> std::io::Result<(u8, Vec<u8>)> {
    loop {
        let (opcode, payload) = ws_read_frame(stream)?;
        match opcode {
            WS_OP_TEXT | WS_OP_BINARY => return Ok((opcode, payload)),
            WS_OP_CLOSE => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "close",
                ))
            }
            WS_OP_PING => {
                let _ = ws_write_frame(stream, WS_OP_PONG, &payload);
            }
            WS_OP_PONG | WS_OP_CONTINUATION => {}
            _ => {}
        }
    }
}

/// Build the connection object exposed to GTS scripts.
pub(crate) fn new_ws_conn_object(conn: Rc<WsConn>) -> Object {
    let obj = Rc::new(RefCell::new(HashData::default()));
    obj.borrow_mut().set(
        WS_CONN_STATE_KEY,
        Object::Hash(Rc::new(RefCell::new(HashData::default()))),
    );

    let c = conn.clone();
    obj.borrow_mut().set(
        "send",
        native("ws.send", move |ctx, args| ws_send_text(ctx, &c, args)),
    );
    let c = conn.clone();
    obj.borrow_mut().set(
        "sendText",
        native("ws.sendText", move |ctx, args| ws_send_text(ctx, &c, args)),
    );
    let c = conn.clone();
    obj.borrow_mut().set(
        "sendBinary",
        native("ws.sendBinary", move |ctx, args| {
            ws_send_binary(ctx, &c, args)
        }),
    );
    let c = conn.clone();
    obj.borrow_mut().set(
        "recv",
        native("ws.recv", move |ctx, args| ws_recv(ctx, &c, args)),
    );
    let c = conn.clone();
    obj.borrow_mut().set(
        "close",
        native("ws.close", move |_ctx, _args| {
            let mut guard = c.stream.borrow_mut();
            *guard = None;
            Object::Undefined
        }),
    );

    Object::Hash(obj)
}

pub(crate) fn ws_send_text(ctx: &mut CallContext, conn: &Rc<WsConn>, args: &[Object]) -> Object {
    let data = match args.first() {
        Some(v) => v.inspect(),
        None => return new_error(ctx.pos.clone(), "ws.send requires data"),
    };
    ws_write(ctx, conn, WS_OP_TEXT, data.into_bytes(), "ws.send")
}

pub(crate) fn ws_send_binary(ctx: &mut CallContext, conn: &Rc<WsConn>, args: &[Object]) -> Object {
    let data = match args.first() {
        Some(v) => v.inspect().into_bytes(),
        None => return new_error(ctx.pos.clone(), "ws.sendBinary requires data"),
    };
    ws_write(ctx, conn, WS_OP_BINARY, data, "ws.sendBinary")
}

pub(crate) fn ws_write(
    ctx: &mut CallContext,
    conn: &Rc<WsConn>,
    opcode: u8,
    payload: Vec<u8>,
    name: &str,
) -> Object {
    let mut guard = conn.stream.borrow_mut();
    let stream = match guard.as_mut() {
        Some(s) => s,
        None => return new_error(ctx.pos.clone(), format!("{}: connection closed", name)),
    };
    match ws_write_frame(stream, opcode, &payload) {
        Ok(_) => Object::Undefined,
        Err(e) => new_error(ctx.pos.clone(), format!("{}: {}", name, e)),
    }
}

pub(crate) fn ws_recv(ctx: &mut CallContext, conn: &Rc<WsConn>, _args: &[Object]) -> Object {
    let mut guard = conn.stream.borrow_mut();
    let stream = match guard.as_mut() {
        Some(s) => s,
        None => return new_error(ctx.pos.clone(), "ws.recv: connection closed"),
    };
    match ws_read_message(stream) {
        Ok((_op, data)) => str_obj(String::from_utf8_lossy(&data).into_owned()),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Object::Null,
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionAborted => Object::Null,
        Err(e) => new_error(ctx.pos.clone(), format!("ws.recv: {}", e)),
    }
}

// --- Server side ----------------------------------------------------------
