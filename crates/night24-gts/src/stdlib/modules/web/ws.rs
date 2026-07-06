use std::cell::RefCell;
use std::io::{Read, Write};
use std::rc::Rc;

use super::super::net_ws_client::{compute_accept_key, new_ws_conn_object, WsConn};
use super::super::signal::exact_match;
use super::helpers::{header_value, inject_route_params};
use super::request::WebRequestOutcome;
use super::WebApp;
use crate::object::{str_obj, CallContext, HashData, Object};
use crate::stdlib::helpers::{call_script_function, ObjectBuilder};

struct TinyWsStream {
    inner: Box<dyn tiny_http::ReadWrite + Send>,
}

impl Read for TinyWsStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for TinyWsStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn web_handle_ws_request(
    ctx: &mut CallContext,
    app: &Rc<WebApp>,
    request: tiny_http::Request,
    method: String,
    url: String,
    path: String,
    remote_addr: String,
    headers_obj: Rc<RefCell<HashData>>,
    query_obj: Rc<RefCell<HashData>>,
    req_segments: &[&str],
) -> Result<WebRequestOutcome, String> {
    let mut matched: Option<(Object, Vec<(String, String)>)> = None;
    let routes = app.routes.borrow();
    for route in routes.iter() {
        if !route.websocket {
            continue;
        }
        let params = match exact_match(&route.segments, req_segments) {
            Some(p) => p,
            None => continue,
        };
        if let Some(handler) = route.handlers.first() {
            matched = Some((handler.clone(), params));
            break;
        }
    }
    drop(routes);

    let Some((handler, params)) = matched else {
        let _ = request.respond(
            tiny_http::Response::from_string("WebSocket endpoint not found").with_status_code(404),
        );
        return Ok(WebRequestOutcome::Responded);
    };

    let Some(key) = header_value(&headers_obj.borrow(), "Sec-WebSocket-Key") else {
        let _ = request.respond(
            tiny_http::Response::from_string("Missing Sec-WebSocket-Key").with_status_code(400),
        );
        return Ok(WebRequestOutcome::Responded);
    };

    let accept = compute_accept_key(&key);
    let mut response = tiny_http::Response::new_empty(tiny_http::StatusCode(101));
    if let Ok(h) = tiny_http::Header::from_bytes(&b"Upgrade"[..], &b"websocket"[..]) {
        response = response.with_header(h);
    }
    if let Ok(h) = tiny_http::Header::from_bytes(&b"Connection"[..], &b"Upgrade"[..]) {
        response = response.with_header(h);
    }
    if let Ok(h) = tiny_http::Header::from_bytes(&b"Sec-WebSocket-Accept"[..], accept.as_bytes()) {
        response = response.with_header(h);
    }

    let stream = request.upgrade("websocket", response);
    let conn = new_ws_conn_object(Rc::new(WsConn {
        stream: RefCell::new(Some(Box::new(TinyWsStream { inner: stream }))),
    }));

    let req_obj = ObjectBuilder::new()
        .set("method", str_obj(method))
        .set("url", str_obj(url))
        .set("path", str_obj(path))
        .set("remoteAddr", str_obj(remote_addr))
        .set("query", Object::Hash(query_obj.clone()))
        .set("headers", Object::Hash(headers_obj.clone()))
        .into_shared();
    inject_route_params(req_obj.clone(), &query_obj, &headers_obj, &params);

    let result = call_script_function(&handler, ctx.env, &[conn, Object::Hash(req_obj)]);
    if result.is_runtime_error() {
        return Err(result.inspect());
    }
    Ok(WebRequestOutcome::Responded)
}
