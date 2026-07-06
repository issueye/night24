use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::helpers::{
    build_headers_object, build_query_object, inject_route_params, is_websocket_upgrade,
};
use super::response::web_respond;
use super::routing::WebRoute;
use super::ws::web_handle_ws_request;
use super::WebApp;
use crate::object::{str_obj, Builtin, CallContext, Object, PromiseState};
use crate::stdlib::helpers::{
    call_script_function, finish_streaming_response, http_response_object, HttpResponseState,
    ObjectBuilder,
};
use crate::stdlib::modules::signal::{exact_match, prefix_match};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WebRequestOutcome {
    Responded,
    Pending,
}

pub(super) struct ActiveStream {
    state: Rc<RefCell<HttpResponseState>>,
    pending_counter: Option<Rc<Cell<usize>>>,
    pending_done: Rc<Cell<bool>>,
}

pub(super) type ActiveStreams = Rc<RefCell<Vec<ActiveStream>>>;

struct MatchedWebHandler {
    handler: Object,
    params: Vec<(String, String)>,
}

/// Process one request: build context, match routes, run the handler chain,
/// then respond on the original request (consumed by value).
pub(super) fn web_handle_request(
    ctx: &mut CallContext,
    app: &Rc<WebApp>,
    mut request: tiny_http::Request,
    pending_responses: Option<Rc<Cell<usize>>>,
    active_streams: Option<ActiveStreams>,
) -> Result<WebRequestOutcome, String> {
    let method = request.method().as_str().to_ascii_uppercase();
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or(&url).to_string();
    let remote_addr = request
        .remote_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();

    let headers_obj = build_headers_object(request.headers());
    let query_obj = build_query_object(&url);

    let req_segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if is_websocket_upgrade(&headers_obj.borrow()) {
        return web_handle_ws_request(
            ctx,
            app,
            request,
            method,
            url,
            path.clone(),
            remote_addr,
            headers_obj,
            query_obj,
            &req_segments,
        );
    }

    // Read body only for ordinary HTTP requests. Upgrade requests must keep
    // the socket intact so tiny_http can hand it over to the WS frame layer.
    let mut body_buf = Vec::new();
    {
        let reader = request.as_reader();
        let _ = reader.read_to_end(&mut body_buf);
    }
    let body = String::from_utf8_lossy(&body_buf).into_owned();

    let request_slot = Rc::new(RefCell::new(Some(request)));
    let resp_state = Rc::new(RefCell::new(HttpResponseState::default()));
    resp_state.borrow_mut().request_slot = Some(request_slot.clone());
    let req_obj = ObjectBuilder::new()
        .set("method", str_obj(method.clone()))
        .set("url", str_obj(url.clone()))
        .set("path", str_obj(path.clone()))
        .set("body", str_obj(body.clone()))
        .set("remoteAddr", str_obj(remote_addr))
        .set("query", Object::Hash(query_obj.clone()))
        .set("headers", Object::Hash(headers_obj.clone()))
        .into_shared();
    let res_obj = http_response_object(resp_state.clone());

    // Collect the chain of handlers to invoke, in route-registration order.
    let routes = app.routes.borrow();
    let chain = matching_handler_chain(&routes, &method, &req_segments);
    drop(routes);

    let handler_error = if chain.is_empty() {
        // No route matched -> 404.
        let mut g = resp_state.borrow_mut();
        g.status = Some(404);
        g.content_type = Some("text/plain".to_string());
        g.body = Some(format!("Not Found: {} {}", method, path).into_bytes());
        None
    } else {
        // Run the matched handler chain. Handlers use Express-style
        // (req, res, next); the old ctx wrapper is intentionally retired.
        let mut err: Option<String> = None;
        for matched in chain {
            inject_route_params(req_obj.clone(), &query_obj, &headers_obj, &matched.params);
            let result = call_script_function(
                &matched.handler,
                ctx.env,
                &[
                    Object::Hash(req_obj.clone()),
                    res_obj.clone(),
                    Object::Builtin(Rc::new(Builtin {
                        name: "web.next".to_string(),
                        func: Rc::new(|_ctx, _args| Object::Undefined),
                        extra: None,
                    })),
                ],
            );
            if result.is_runtime_error() {
                err = Some(result.inspect());
                break;
            }
            match web_handle_handler_promise(
                &result,
                resp_state.clone(),
                request_slot.clone(),
                pending_responses.clone(),
                active_streams.clone(),
            ) {
                WebPromiseOutcome::NotPromise => {}
                WebPromiseOutcome::Rejected(msg) => {
                    err = Some(msg);
                    break;
                }
                WebPromiseOutcome::Pending => {
                    return Ok(WebRequestOutcome::Pending);
                }
            }
            if resp_state.borrow().body.is_some() {
                break;
            }
        }
        err
    };

    // If a handler threw, override the response with a 500.
    if let Some(msg) = handler_error {
        let mut g = resp_state.borrow_mut();
        g.status = Some(500);
        g.content_type = Some("text/plain".to_string());
        g.body = Some(format!("Internal Server Error: {}", msg).into_bytes());
    }

    if let Some(request) = request_slot.borrow_mut().take() {
        web_respond(request, &resp_state);
    }
    Ok(WebRequestOutcome::Responded)
}

fn matching_handler_chain(
    routes: &[WebRoute],
    method: &str,
    req_segments: &[&str],
) -> Vec<MatchedWebHandler> {
    let mut chain = Vec::new();
    for route in routes {
        if route.websocket {
            continue;
        }
        if route.method != "ALL" && route.method != "USE" && route.method != method {
            continue;
        }
        let params = if route.method == "USE" {
            match prefix_match(&route.segments, req_segments) {
                Some(params) => params,
                None => continue,
            }
        } else {
            match exact_match(&route.segments, req_segments) {
                Some(params) => params,
                None => continue,
            }
        };
        for handler in &route.handlers {
            chain.push(MatchedWebHandler {
                handler: handler.clone(),
                params: params.clone(),
            });
        }
    }
    chain
}

enum WebPromiseOutcome {
    NotPromise,
    Pending,
    Rejected(String),
}

fn web_handle_handler_promise(
    result: &Object,
    resp_state: Rc<RefCell<HttpResponseState>>,
    request_slot: Rc<RefCell<Option<tiny_http::Request>>>,
    pending_responses: Option<Rc<Cell<usize>>>,
    active_streams: Option<ActiveStreams>,
) -> WebPromiseOutcome {
    let Object::Promise(promise) = result else {
        return WebPromiseOutcome::NotPromise;
    };
    match promise.state() {
        PromiseState::Pending => {
            let pending_done = Rc::new(Cell::new(false));
            if let Some(counter) = pending_responses.as_ref() {
                counter.set(counter.get() + 1);
            }
            let counter_for_completion = pending_responses.clone();
            let done_for_completion = pending_done.clone();
            let resp_state_for_completion = resp_state.clone();
            promise.add_continuation(Box::new(move |state, value| {
                if state == PromiseState::Rejected || value.is_runtime_error() {
                    let mut g = resp_state_for_completion.borrow_mut();
                    g.status = Some(500);
                    g.content_type = Some("text/plain".to_string());
                    g.body =
                        Some(format!("Internal Server Error: {}", value.inspect()).into_bytes());
                }
                if let Some(request) = request_slot.borrow_mut().take() {
                    web_respond(request, &resp_state_for_completion);
                } else {
                    let _ = finish_streaming_response(&resp_state_for_completion);
                }
                if !done_for_completion.get() {
                    done_for_completion.set(true);
                    if let Some(counter) = counter_for_completion.as_ref() {
                        counter.set(counter.get().saturating_sub(1));
                    }
                }
            }));
            if let Some(active) = active_streams {
                active.borrow_mut().push(ActiveStream {
                    state: resp_state,
                    pending_counter: pending_responses,
                    pending_done,
                });
            }
            WebPromiseOutcome::Pending
        }
        PromiseState::Rejected => {
            WebPromiseOutcome::Rejected(promise.value().unwrap_or(Object::Undefined).inspect())
        }
        PromiseState::Fulfilled => {
            let value = promise.value().unwrap_or(Object::Undefined);
            if value.is_runtime_error() {
                WebPromiseOutcome::Rejected(value.inspect())
            } else {
                WebPromiseOutcome::NotPromise
            }
        }
    }
}

pub(super) fn poll_active_streams(active_streams: &ActiveStreams) {
    let now = std::time::Instant::now();
    active_streams.borrow_mut().retain(|entry| {
        if entry.pending_done.get() {
            return false;
        }
        let deadline = entry.state.borrow().stream_deadline;
        let Some(deadline) = deadline else {
            return true;
        };
        if deadline > now {
            return true;
        }
        let _ = finish_streaming_response(&entry.state);
        if !entry.pending_done.get() {
            entry.pending_done.set(true);
            if let Some(counter) = entry.pending_counter.as_ref() {
                counter.set(counter.get().saturating_sub(1));
            }
        }
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stdlib::helpers::native;

    fn handler(name: &str) -> Object {
        native(name, |_ctx, _args| Object::Undefined)
    }

    fn route(method: &str, segments: &[&str], handlers: Vec<Object>) -> WebRoute {
        WebRoute {
            method: method.to_string(),
            segments: segments.iter().map(|segment| segment.to_string()).collect(),
            handlers,
            websocket: false,
        }
    }

    fn websocket_route(method: &str, segments: &[&str], handlers: Vec<Object>) -> WebRoute {
        WebRoute {
            websocket: true,
            ..route(method, segments, handlers)
        }
    }

    fn req_segments(path: &str) -> Vec<&str> {
        path.split('/')
            .filter(|segment| !segment.is_empty())
            .collect()
    }

    fn handler_names(chain: &[MatchedWebHandler]) -> Vec<String> {
        chain
            .iter()
            .map(|matched| match &matched.handler {
                Object::Builtin(handler) => handler.name.clone(),
                other => other.inspect(),
            })
            .collect()
    }

    #[test]
    fn matching_handler_chain_collects_middleware_prefix_params() {
        let routes = vec![route("USE", &["api", ":tenant"], vec![handler("mw")])];
        let segments = req_segments("/api/acme/users");

        let chain = matching_handler_chain(&routes, "GET", &segments);

        assert_eq!(handler_names(&chain), vec!["mw"]);
        assert_eq!(
            chain[0].params,
            vec![("tenant".to_string(), "acme".to_string())]
        );
    }

    #[test]
    fn matching_handler_chain_filters_by_method() {
        let routes = vec![
            route("POST", &["items"], vec![handler("post")]),
            route("GET", &["items"], vec![handler("get")]),
        ];
        let segments = req_segments("/items");

        let chain = matching_handler_chain(&routes, "GET", &segments);

        assert_eq!(handler_names(&chain), vec!["get"]);
    }

    #[test]
    fn matching_handler_chain_matches_all_with_exact_path() {
        let routes = vec![
            route("ALL", &["items", ":id"], vec![handler("all")]),
            route("ALL", &["items"], vec![handler("prefix-not-all")]),
        ];
        let segments = req_segments("/items/42");

        let chain = matching_handler_chain(&routes, "PATCH", &segments);

        assert_eq!(handler_names(&chain), vec!["all"]);
        assert_eq!(chain[0].params, vec![("id".to_string(), "42".to_string())]);
    }

    #[test]
    fn matching_handler_chain_skips_websocket_routes() {
        let routes = vec![
            websocket_route("GET", &["socket"], vec![handler("ws")]),
            route("GET", &["socket"], vec![handler("http")]),
        ];
        let segments = req_segments("/socket");

        let chain = matching_handler_chain(&routes, "GET", &segments);

        assert_eq!(handler_names(&chain), vec!["http"]);
    }

    #[test]
    fn matching_handler_chain_preserves_route_and_handler_order() {
        let routes = vec![
            route("USE", &[], vec![handler("first"), handler("second")]),
            route("GET", &["items"], vec![handler("third")]),
            route("ALL", &["items"], vec![handler("fourth"), handler("fifth")]),
        ];
        let segments = req_segments("/items");

        let chain = matching_handler_chain(&routes, "GET", &segments);

        assert_eq!(
            handler_names(&chain),
            vec!["first", "second", "third", "fourth", "fifth"]
        );
    }
}
