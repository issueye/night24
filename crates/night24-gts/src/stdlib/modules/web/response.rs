use std::cell::RefCell;
use std::rc::Rc;

use crate::stdlib::helpers::HttpResponseState;

pub(super) fn web_respond(
    request: tiny_http::Request,
    resp_state: &Rc<RefCell<HttpResponseState>>,
) {
    // D1.1: If the handler entered streaming mode (res.begin + res.write +
    // res.flush), the response was already sent over the wire. Skip the
    // buffered respond path.
    if resp_state.borrow().stream_writer.is_some() {
        // The writer is dropped here, which triggers tiny_http's
        // "responded" notification (keep-alive bookkeeping).
        drop(request);
        return;
    }
    let state = resp_state.borrow();
    let status_code = state.status.unwrap_or(200);
    let body_bytes = state.body.clone().unwrap_or_default();
    let content_type = state
        .content_type
        .clone()
        .unwrap_or_else(|| "text/plain".to_string());
    let mut response = tiny_http::Response::from_data(body_bytes);
    response = response.with_status_code(tiny_http::StatusCode(status_code));
    if let Ok(h) = tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()) {
        response = response.with_header(h);
    }
    for (k, v) in &state.headers {
        if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            response = response.with_header(h);
        }
    }
    if let Ok(h) = tiny_http::Header::from_bytes(&b"Connection"[..], &b"close"[..]) {
        response = response.with_header(h);
    }
    drop(state);
    let _ = request.respond(response);
}
