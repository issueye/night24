use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use super::super::helpers::*;
use crate::object::{new_error, num_obj, str_obj, CallContext, HashData, Object};

/// The synchronous VM has no event loop, so a Go-style background accept loop
/// cannot be reproduced. We expose `createServer(handler?, port?)` returning a
/// server object whose `acceptOne(handler?)` blocks for a single request,
/// invokes the handler synchronously with `{method,url,path,body,query,headers,
/// remoteAddr}` and a response object `{status,setHeader,send,json,end}`, then
/// returns. The handler fully controls the response via the closure-captured
/// `tiny_http::Response` builder state.
const HTTP_SERVER_STATE_KEY: &str = "__http_server__";

pub(crate) struct HttpServer {
    server: std::cell::RefCell<Option<tiny_http::Server>>,
    handler: std::cell::RefCell<Option<Object>>,
}

pub(crate) fn http_server_module() -> Object {
    module(vec![(
        "createServer",
        native("http.createServer", http_create_server),
    )])
}

pub(crate) fn http_create_server(ctx: &mut CallContext, args: &[Object]) -> Object {
    // Args mirror the Go signature loosely: (handler?, port?).
    //   http.createServer(handler)            — handler only, ephemeral port
    //   http.createServer(handler, port)      — handler + port
    //   http.createServer(port)               — port only, handler via acceptOne
    let mut handler = None;
    let mut port: Option<u16> = None;
    for arg in args {
        match arg {
            value if is_callable(value) => handler = Some(arg.clone()),
            Object::Number(n) => port = Some(*n as u16),
            _ => {}
        }
    }

    let bind_addr = match port {
        Some(p) => format!("0.0.0.0:{}", p),
        None => "0.0.0.0:0".to_string(), // ephemeral port on all interfaces
    };
    let server = match tiny_http::Server::http(bind_addr.as_str()) {
        Ok(s) => s,
        Err(e) => return new_error(ctx.pos.clone(), format!("http.createServer: {}", e)),
    };
    let bound_addr = server.server_addr();
    let bound_port = match bound_addr {
        tiny_http::ListenAddr::IP(addr) => addr.port(),
    };

    let srv = Rc::new(HttpServer {
        server: std::cell::RefCell::new(Some(server)),
        handler: std::cell::RefCell::new(handler),
    });
    let obj = ObjectBuilder::new()
        .set(HTTP_SERVER_STATE_KEY, ObjectBuilder::new().build())
        .set("port", num_obj(bound_port as f64))
        .set("address", str_obj(format!(":{}", bound_port)))
        .into_shared();

    let s = srv.clone();
    obj.borrow_mut().set(
        "acceptOne",
        native("server.acceptOne", move |ctx, args| {
            http_accept_one(ctx, &s, args)
        }),
    );
    let s = srv.clone();
    obj.borrow_mut().set(
        "accept",
        native("server.accept", move |ctx, args| {
            http_accept_one(ctx, &s, args)
        }),
    );
    let s = srv.clone();
    obj.borrow_mut().set(
        "close",
        native("server.close", move |_ctx, _args| {
            let mut guard = s.server.borrow_mut();
            *guard = None; // drop the tiny_http::Server
            Object::Undefined
        }),
    );

    Object::Hash(obj)
}

pub(crate) fn http_accept_one(
    ctx: &mut CallContext,
    server: &Rc<HttpServer>,
    args: &[Object],
) -> Object {
    let handler = match args.first() {
        Some(value) if is_callable(value) => Some(value.clone()),
        _ => server.handler.borrow().clone(),
    };

    // Take the request out of the server, then run the handler. We must drop
    // the listener borrow before invoking the handler so the handler can call
    // close()/acceptOne() recursively without RefCell reentrancy.
    let mut request = {
        let guard = server.server.borrow();
        let srv = match guard.as_ref() {
            Some(s) => s,
            None => return new_error(ctx.pos.clone(), "server.acceptOne: server closed"),
        };
        // tiny_http's recv() blocks until a request arrives.
        match srv.recv() {
            Ok(r) => r,
            Err(e) => return new_error(ctx.pos.clone(), format!("server.acceptOne: {}", e)),
        }
    };

    // Build the request object.
    let method = request.method().as_str().to_string();
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or(&url).to_string();
    let remote_addr = request
        .remote_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();

    // Collect headers into a Hash (first value per name).
    let headers_obj = build_headers_object(request.headers());

    // Parse query string into a Hash.
    let query_obj = build_query_object(&url);

    // Read the body.
    let mut body_buf = Vec::new();
    {
        let reader = request.as_reader();
        let _ = reader.read_to_end(&mut body_buf);
    }
    let body = String::from_utf8_lossy(&body_buf).into_owned();

    // Response state shared with the handler closures.
    let resp_state = Rc::new(RefCell::new(HttpResponseState::default()));

    let req_obj =
        build_request_object(method, url, path, body, query_obj, headers_obj, remote_addr);

    let res_obj = http_response_object(resp_state.clone());

    // Invoke handler(req, res). The handler populates resp_state via closures.
    let handler_result = match handler {
        Some(h) => call_script_function(&h, ctx.env, &[Object::Hash(req_obj), res_obj.clone()]),
        None => Object::Undefined,
    };

    // If the handler threw a runtime error, respond with 500 and surface it.
    if handler_result.is_runtime_error() {
        let _ = request.respond(
            tiny_http::Response::from_string("Internal Server Error").with_status_code(500),
        );
        return handler_result;
    }

    // Build the tiny_http::Response from the accumulated state and respond on
    // the original request (kept alive above).
    let state = resp_state.borrow();
    let status_code = state.status.unwrap_or(200);
    let tiny_status = tiny_http::StatusCode(status_code);
    let body_bytes = state.body.clone().unwrap_or_default();
    let content_type = state
        .content_type
        .clone()
        .unwrap_or_else(|| "text/plain".to_string());
    let mut response = tiny_http::Response::from_data(body_bytes);
    response = response.with_status_code(tiny_status);
    if let Ok(h) = tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()) {
        response = response.with_header(h);
    }
    for (k, v) in &state.headers {
        if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            response = response.with_header(h);
        }
    }
    drop(state);
    let _ = request.respond(response);

    Object::Undefined
}

fn build_headers_object(headers: &[tiny_http::Header]) -> Rc<RefCell<HashData>> {
    let mut builder = ObjectBuilder::new();
    let mut seen: HashSet<String> = HashSet::new();
    for h in headers {
        let key = h.field.as_str().to_string();
        if seen.insert(key.to_ascii_lowercase()) {
            builder.insert(key, str_obj(h.value.as_str().to_string()));
        }
    }
    builder.into_shared()
}

fn build_query_object(url: &str) -> Rc<RefCell<HashData>> {
    let mut builder = ObjectBuilder::new();
    if let Some(qstart) = url.find('?') {
        for pair in url[qstart + 1..].split('&') {
            if let Some(eq) = pair.find('=') {
                builder.insert(
                    percent_decode(&pair[..eq]),
                    str_obj(percent_decode(&pair[eq + 1..])),
                );
            } else if !pair.is_empty() {
                builder.insert(percent_decode(pair), str_obj(String::new()));
            }
        }
    }
    builder.into_shared()
}

fn build_request_object(
    method: String,
    url: String,
    path: String,
    body: String,
    query: Rc<RefCell<HashData>>,
    headers: Rc<RefCell<HashData>>,
    remote_addr: String,
) -> Rc<RefCell<HashData>> {
    ObjectBuilder::new()
        .set("method", str_obj(method))
        .set("url", str_obj(url))
        .set("path", str_obj(path))
        .set("body", str_obj(body))
        .set("query", Object::Hash(query))
        .set("headers", Object::Hash(headers))
        .set("remoteAddr", str_obj(remote_addr))
        .into_shared()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string_field(hash: &HashData, key: &str) -> Option<String> {
        match hash.get(key) {
            Some(Object::String(value)) => Some(value.to_string()),
            _ => None,
        }
    }

    #[test]
    fn build_query_object_decodes_values_and_empty_flags() {
        let query = build_query_object("/items?q=hello%20world&empty=&flag");
        let query = query.borrow();

        assert_eq!(string_field(&query, "q").as_deref(), Some("hello world"));
        assert_eq!(string_field(&query, "empty").as_deref(), Some(""));
        assert_eq!(string_field(&query, "flag").as_deref(), Some(""));
    }

    #[test]
    fn build_headers_object_keeps_first_case_insensitive_header() {
        let headers = vec![
            tiny_http::Header::from_bytes(&b"X-Test"[..], &b"first"[..]).unwrap(),
            tiny_http::Header::from_bytes(&b"x-test"[..], &b"second"[..]).unwrap(),
        ];

        let out = build_headers_object(&headers);
        let out = out.borrow();

        assert_eq!(out.entries.len(), 1);
        assert_eq!(string_field(&out, "X-Test").as_deref(), Some("first"));
    }

    #[test]
    fn build_request_object_sets_expected_views() {
        let query = build_query_object("/items?id=42");
        let headers = build_headers_object(&[tiny_http::Header::from_bytes(
            &b"Accept"[..],
            &b"text/plain"[..],
        )
        .unwrap()]);

        let req = build_request_object(
            "GET".to_string(),
            "/items?id=42".to_string(),
            "/items".to_string(),
            "body".to_string(),
            query,
            headers,
            "127.0.0.1:1000".to_string(),
        );
        let req = req.borrow();

        assert_eq!(string_field(&req, "method").as_deref(), Some("GET"));
        assert_eq!(string_field(&req, "path").as_deref(), Some("/items"));
        assert!(matches!(req.get("query"), Some(Object::Hash(_))));
        assert!(matches!(req.get("headers"), Some(Object::Hash(_))));
        assert_eq!(
            string_field(&req, "remoteAddr").as_deref(),
            Some("127.0.0.1:1000")
        );
    }
}
