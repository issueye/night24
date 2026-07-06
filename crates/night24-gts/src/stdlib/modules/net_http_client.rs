#[cfg(feature = "tokio")]
use std::sync::OnceLock;

use super::super::helpers::*;
use super::stream::stream_from_text_object;
use crate::object::{
    bool_obj, new_error, num_obj, str_obj, AsyncCompletionData, AsyncHttpResponse, CallContext,
    HashData, Object, Promise,
};

#[cfg(feature = "tokio")]
use reqwest::Method;

pub(crate) fn http_client_module() -> Object {
    module(vec![
        ("get", native("http.get", http_client_get)),
        ("post", native("http.post", http_client_post)),
        ("request", native("http.request", http_client_request)),
        (
            "requestAsync",
            native("http.requestAsync", http_client_request_async),
        ),
        ("stream", native("http.stream", http_client_stream)),
        (
            "streamAsync",
            native("http.streamAsync", http_client_stream_async),
        ),
        ("fetch", native("http.fetch", http_client_request)),
    ])
}

#[derive(Debug, Clone)]
struct OwnedHttpRequest {
    url: String,
    method: String,
    body: Option<String>,
    headers: Vec<(String, String)>,
    timeout_ms: Option<u64>,
    /// Optional proxy URL (e.g. "http://127.0.0.1:7890"). When set, the
    /// request is routed through the proxy on both the ureq and reqwest paths.
    proxy: Option<String>,
}

#[cfg(feature = "tokio")]
#[derive(Debug)]
struct HttpClientState {
    runtime: tokio::runtime::Runtime,
    client: reqwest::Client,
}

#[cfg(feature = "tokio")]
static HTTP_CLIENT_STATE: OnceLock<HttpClientState> = OnceLock::new();

#[cfg(feature = "tokio")]
fn http_client_state() -> &'static HttpClientState {
    HTTP_CLIENT_STATE.get_or_init(|| HttpClientState {
        runtime: tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("gts-http-client")
            .enable_all()
            .build()
            .expect("build http client runtime"),
        client: reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .pool_max_idle_per_host(8)
            .tcp_keepalive(Some(std::time::Duration::from_secs(30)))
            .build()
            .expect("build reqwest client"),
    })
}

/// Cache of reqwest clients keyed by proxy URL, so repeated requests through
/// the same proxy reuse a single pooled client. The default (no-proxy) client
/// lives in `http_client_state()`; this cache only holds proxy-specific ones.
#[cfg(feature = "tokio")]
static PROXY_CLIENTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, reqwest::Client>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Return a reqwest client: the global default when proxy is None, or a cached
/// proxy-configured client otherwise. Each distinct proxy URL gets its own
/// pooled client (built once with the same pool settings as the default).
#[cfg(feature = "tokio")]
fn reqwest_client_for_proxy(proxy: &Option<String>) -> reqwest::Client {
    // No proxy: reuse the global default client.
    let proxy_url = match proxy {
        None => return http_client_state().client.clone(),
        Some(p) => p.clone(),
    };

    let mut cache = PROXY_CLIENTS.lock().expect("proxy client cache poisoned");
    if let Some(client) = cache.get(&proxy_url) {
        return client.clone();
    }

    let proxy = match reqwest::Proxy::all(&proxy_url) {
        Ok(p) => p,
        Err(e) => {
            // Fall back to the default client on an invalid proxy URL rather
            // than panicking; the request will likely fail with a clearer
            // network error.
            eprintln!("invalid proxy '{}': {}", proxy_url, e);
            return http_client_state().client.clone();
        }
    };
    let client = reqwest::Client::builder()
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .pool_max_idle_per_host(8)
        .tcp_keepalive(Some(std::time::Duration::from_secs(30)))
        .proxy(proxy)
        .build()
        .expect("build reqwest proxy client");
    cache.insert(proxy_url, client.clone());
    client
}

pub(crate) fn http_client_get(ctx: &mut CallContext, args: &[Object]) -> Object {
    let url = match args.first() {
        Some(Object::String(s)) => s.to_string(),
        Some(Object::Hash(h)) => match h.borrow().get("url") {
            Some(Object::String(s)) => s.to_string(),
            _ => return new_error(ctx.pos.clone(), "http.get: url is required"),
        },
        _ => {
            return new_error(
                ctx.pos.clone(),
                "http.get: requires a URL string or options object",
            )
        }
    };

    // Optional proxy from the options object.
    let proxy = match args.first() {
        Some(Object::Hash(h)) => match h.borrow().get("proxy") {
            Some(Object::String(s)) if !s.is_empty() => Some(s.to_string()),
            _ => None,
        },
        _ => None,
    };

    let agent = match ureq_agent(proxy.as_deref()) {
        Ok(a) => a,
        Err(e) => return new_error(ctx.pos.clone(), format!("http.get: {}", e)),
    };
    let mut req = agent.get(&url);

    // Apply headers if provided
    if let Some(Object::Hash(opts)) = args.first() {
        if let Some(Object::Hash(headers)) = opts.borrow().get("headers") {
            let headers_data = headers.borrow();
            for (key, value) in &headers_data.entries {
                let v = value_to_string(value);
                req = req.set(key, &v);
            }
        }
    }

    match req.call() {
        Ok(response) => build_http_response(response),
        Err(ureq::Error::Status(code, response)) => build_http_response_with_status(response, code),
        Err(e) => new_error(ctx.pos.clone(), format!("http.get: {}", e)),
    }
}

pub(crate) fn http_client_post(ctx: &mut CallContext, args: &[Object]) -> Object {
    let (url, body, content_type, proxy) = match args.first() {
        Some(Object::String(s)) => {
            let body = args.get(1).map(http_body_to_string).unwrap_or_default();
            let ct = if matches!(args.get(1), Some(Object::Hash(_))) {
                "application/json"
            } else {
                "text/plain"
            };
            (s.to_string(), body, ct, None)
        }
        Some(Object::Hash(h)) => {
            let url = match h.borrow().get("url") {
                Some(Object::String(s)) => s.to_string(),
                _ => return new_error(ctx.pos.clone(), "http.post: url is required"),
            };
            let body = h
                .borrow()
                .get("body")
                .map(http_body_to_string)
                .unwrap_or_default();
            let ct = "application/json";
            let proxy = match h.borrow().get("proxy") {
                Some(Object::String(s)) if !s.is_empty() => Some(s.to_string()),
                _ => None,
            };
            (url, body, ct, proxy)
        }
        _ => {
            return new_error(
                ctx.pos.clone(),
                "http.post: requires a URL string or options object",
            )
        }
    };

    let agent = match ureq_agent(proxy.as_deref()) {
        Ok(a) => a,
        Err(e) => return new_error(ctx.pos.clone(), format!("http.post: {}", e)),
    };

    match agent
        .post(&url)
        .set("Content-Type", content_type)
        .send_string(&body)
    {
        Ok(response) => build_http_response(response),
        Err(ureq::Error::Status(code, response)) => build_http_response_with_status(response, code),
        Err(e) => new_error(ctx.pos.clone(), format!("http.post: {}", e)),
    }
}

pub(crate) fn http_client_request(ctx: &mut CallContext, args: &[Object]) -> Object {
    let request = match owned_http_request_from_args(ctx, "http.request", args) {
        Ok(request) => request,
        Err(err) => return err,
    };

    match perform_owned_http_request(request) {
        Ok(response) => async_http_response_to_object(response),
        Err(e) => new_error(ctx.pos.clone(), format!("http.request: {}", e)),
    }
}

pub(crate) fn http_client_request_async(ctx: &mut CallContext, args: &[Object]) -> Object {
    let request = match owned_http_request_from_args(ctx, "http.requestAsync", args) {
        Ok(request) => request,
        Err(err) => {
            let promise = Promise::new();
            promise.reject(err);
            return Object::Promise(promise);
        }
    };

    let vm = ctx.vm();
    let (id, promise) = vm.create_async_completion_promise();
    let sender = vm.async_completion_sender();
    #[cfg(feature = "tokio")]
    {
        let state = http_client_state();
        let client = state.client.clone();
        state.runtime.spawn(async move {
            match perform_owned_http_request_tokio(client, request).await {
                Ok(response) => sender.resolve(id, AsyncCompletionData::HttpResponse(response)),
                Err(e) => sender.reject(id, format!("http.requestAsync: {}", e)),
            }
        });
    }
    #[cfg(not(feature = "tokio"))]
    {
        std::thread::spawn(move || match perform_owned_http_request(request) {
            Ok(response) => sender.resolve(id, AsyncCompletionData::HttpResponse(response)),
            Err(e) => sender.reject(id, format!("http.requestAsync: {}", e)),
        });
    }

    Object::Promise(promise)
}

pub(crate) fn http_client_stream_async(ctx: &mut CallContext, args: &[Object]) -> Object {
    let request = match owned_http_request_from_args(ctx, "http.streamAsync", args) {
        Ok(request) => request,
        Err(err) => {
            let promise = Promise::new();
            promise.reject(err);
            return Object::Promise(promise);
        }
    };

    let vm = ctx.vm();
    let (id, promise) = vm.create_async_completion_promise();
    let sender = vm.async_completion_sender();
    #[cfg(feature = "tokio")]
    {
        let state = http_client_state();
        let client = state.client.clone();
        state.runtime.spawn(async move {
            match perform_owned_http_request_tokio(client, request).await {
                Ok(response) => {
                    sender.resolve(id, AsyncCompletionData::HttpStreamResponse(response))
                }
                Err(e) => sender.reject(id, format!("http.streamAsync: {}", e)),
            }
        });
    }
    #[cfg(not(feature = "tokio"))]
    {
        std::thread::spawn(move || match perform_owned_http_request(request) {
            Ok(response) => sender.resolve(id, AsyncCompletionData::HttpStreamResponse(response)),
            Err(e) => sender.reject(id, format!("http.streamAsync: {}", e)),
        });
    }

    Object::Promise(promise)
}

fn owned_http_request_from_args(
    ctx: &mut CallContext,
    name: &str,
    args: &[Object],
) -> Result<OwnedHttpRequest, Object> {
    match args.first() {
        Some(Object::Hash(h)) => {
            let opts = h.borrow();
            owned_http_request_from_options(name, &opts)
                .map_err(|message| new_error(ctx.pos.clone(), message))
        }
        Some(Object::String(url)) => Ok(OwnedHttpRequest {
            url: url.to_string(),
            method: "GET".to_string(),
            body: None,
            headers: Vec::new(),
            timeout_ms: None,
            proxy: None,
        }),
        _ => {
            return Err(new_error(
                ctx.pos.clone(),
                format!("{name}: requires an options object or URL string"),
            ))
        }
    }
}

fn owned_http_request_from_options(
    name: &str,
    opts: &HashData,
) -> Result<OwnedHttpRequest, String> {
    let view = ObjectView::new(opts);

    let url = match hash_string(opts, "url") {
        Some(url) => url,
        _ => return Err(format!("{name}: url is required")),
    };

    let method = hash_string(opts, "method")
        .map(|value| value.to_uppercase())
        .unwrap_or_else(|| "GET".to_string());

    let body = opts.get("body").map(http_body_to_string);

    let timeout_ms = view
        .number("timeoutMs")
        .filter(|ms| *ms > 0.0)
        .map(|ms| ms as u64);

    let headers = http_headers_from_options(opts);
    let proxy = hash_string(opts, "proxy").filter(|value| !value.is_empty());

    Ok(OwnedHttpRequest {
        url,
        method,
        body,
        headers,
        timeout_ms,
        proxy,
    })
}

fn http_headers_from_options(opts: &HashData) -> Vec<(String, String)> {
    match opts.get("headers") {
        Some(Object::Hash(headers_obj)) => {
            let headers_data = headers_obj.borrow();
            headers_data
                .entries
                .iter()
                .map(|(key, value)| (key.clone(), value_to_string(value)))
                .collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_http_request_from_options_reads_supported_fields() {
        let headers = ObjectBuilder::new()
            .set("Accept", str_obj("application/json".to_string()))
            .set("X-Retry", Object::Number(2.0))
            .build();
        let opts = ObjectBuilder::new()
            .set("url", str_obj("https://example.test/api".to_string()))
            .set("method", str_obj("post".to_string()))
            .set("body", str_obj("payload".to_string()))
            .set("timeoutMs", Object::Number(1500.0))
            .set("headers", headers)
            .set("proxy", str_obj("http://127.0.0.1:7890".to_string()))
            .build();

        let Object::Hash(hash) = opts else {
            panic!("expected hash options");
        };
        let request = owned_http_request_from_options("http.request", &hash.borrow()).unwrap();

        assert_eq!(request.url, "https://example.test/api");
        assert_eq!(request.method, "POST");
        assert_eq!(request.body.as_deref(), Some("payload"));
        assert_eq!(request.timeout_ms, Some(1500));
        assert_eq!(
            request.headers,
            vec![
                ("Accept".to_string(), "application/json".to_string()),
                ("X-Retry".to_string(), "2".to_string())
            ]
        );
        assert_eq!(request.proxy.as_deref(), Some("http://127.0.0.1:7890"));
    }

    #[test]
    fn owned_http_request_from_options_keeps_defaults_for_unsupported_fields() {
        let opts = ObjectBuilder::new()
            .set("url", str_obj("https://example.test".to_string()))
            .set("method", Object::Number(1.0))
            .set("timeoutMs", Object::Number(0.0))
            .set("headers", Object::Boolean(true))
            .set("proxy", str_obj(String::new()))
            .build();

        let Object::Hash(hash) = opts else {
            panic!("expected hash options");
        };
        let request = owned_http_request_from_options("http.request", &hash.borrow()).unwrap();

        assert_eq!(request.method, "GET");
        assert_eq!(request.timeout_ms, None);
        assert!(request.headers.is_empty());
        assert_eq!(request.proxy, None);
    }

    #[test]
    fn owned_http_request_from_options_requires_string_url() {
        let opts = ObjectBuilder::new()
            .set("url", Object::Boolean(true))
            .build();

        let Object::Hash(hash) = opts else {
            panic!("expected hash options");
        };
        let err = owned_http_request_from_options("http.request", &hash.borrow()).unwrap_err();

        assert_eq!(err, "http.request: url is required");
    }
}

fn perform_owned_http_request(request: OwnedHttpRequest) -> Result<AsyncHttpResponse, String> {
    #[cfg(feature = "tokio")]
    {
        let state = http_client_state();
        let client = reqwest_client_for_proxy(&request.proxy);
        state
            .runtime
            .block_on(perform_owned_http_request_tokio(client, request))
    }
    #[cfg(not(feature = "tokio"))]
    {
        return perform_owned_http_request_ureq(request);
    }
}

/// Build a ureq Agent, optionally configured with an HTTP/HTTPS proxy.
/// A proxy URL of None produces a default agent (equivalent to bare
/// ureq::get/post/request).
fn ureq_agent(proxy: Option<&str>) -> Result<ureq::Agent, String> {
    match proxy {
        None => Ok(ureq::AgentBuilder::new().build()),
        Some(url) => {
            let proxy = ureq::Proxy::new(url).map_err(|e| e.to_string())?;
            Ok(ureq::AgentBuilder::new().proxy(proxy).build())
        }
    }
}

#[cfg(not(feature = "tokio"))]
fn perform_owned_http_request_ureq(request: OwnedHttpRequest) -> Result<AsyncHttpResponse, String> {
    let agent = ureq_agent(request.proxy.as_deref())?;
    let mut req = agent.request(&request.method, &request.url);
    for (key, value) in request.headers {
        req = req.set(&key, &value);
    }
    if let Some(timeout_ms) = request.timeout_ms {
        req = req.timeout(std::time::Duration::from_millis(timeout_ms));
    }

    let result = if let Some(body_str) = request.body {
        req.send_string(&body_str)
    } else {
        req.call()
    };

    match result {
        Ok(response) => Ok(async_http_response_from_ureq(response, None)),
        Err(ureq::Error::Status(code, response)) => {
            Ok(async_http_response_from_ureq(response, Some(code)))
        }
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(feature = "tokio")]
async fn perform_owned_http_request_tokio(
    client: reqwest::Client,
    request: OwnedHttpRequest,
) -> Result<AsyncHttpResponse, String> {
    let method = Method::from_bytes(request.method.as_bytes()).map_err(|e| e.to_string())?;
    let mut builder = client.request(method, &request.url);

    for (key, value) in request.headers {
        builder = builder.header(key, value);
    }
    if let Some(timeout_ms) = request.timeout_ms {
        builder = builder.timeout(std::time::Duration::from_millis(timeout_ms));
    }
    if let Some(body) = request.body {
        builder = builder.body(body);
    }

    let response = builder.send().await.map_err(|e| e.to_string())?;
    async_http_response_from_reqwest(response).await
}

#[cfg(feature = "tokio")]
async fn async_http_response_from_reqwest(
    response: reqwest::Response,
) -> Result<AsyncHttpResponse, String> {
    let status = response.status().as_u16();
    let status_text = response
        .status()
        .canonical_reason()
        .unwrap_or("")
        .to_string();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                value.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    let body = response.bytes().await.map_err(|e| e.to_string())?.to_vec();

    Ok(AsyncHttpResponse {
        status,
        status_text,
        headers,
        body,
    })
}

#[cfg(not(feature = "tokio"))]
fn async_http_response_from_ureq(
    response: ureq::Response,
    status_override: Option<u16>,
) -> AsyncHttpResponse {
    let status = status_override.unwrap_or_else(|| response.status());
    let status_text = response.status_text().to_string();
    let headers = response
        .headers_names()
        .into_iter()
        .filter_map(|name| {
            response
                .header(&name)
                .map(|value| (name, value.to_string()))
        })
        .collect();
    let body = match response.into_string() {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };

    AsyncHttpResponse {
        status,
        status_text,
        headers,
        body,
    }
}

fn async_http_response_to_object(response: AsyncHttpResponse) -> Object {
    crate::object::http_stream::http_response_to_object(response)
}

pub(crate) fn http_client_stream(ctx: &mut CallContext, args: &[Object]) -> Object {
    let request = match args.first() {
        Some(Object::Hash(opts)) => {
            let opts = opts.borrow();
            match owned_http_request_from_options("http.stream", &opts) {
                Ok(request) => request,
                Err(message) => return new_error(ctx.pos.clone(), message),
            }
        }
        _ => return new_error(ctx.pos.clone(), "http.stream: requires an options object"),
    };

    let agent = match ureq_agent(request.proxy.as_deref()) {
        Ok(a) => a,
        Err(e) => return new_error(ctx.pos.clone(), format!("http.stream: {}", e)),
    };
    let mut req = agent.request(&request.method, &request.url);

    if let Some(timeout_ms) = request.timeout_ms {
        req = req.timeout(std::time::Duration::from_millis(timeout_ms));
    }

    for (key, value) in request.headers {
        req = req.set(&key, &value);
    }

    let result = if let Some(body_str) = request.body {
        req.send_string(&body_str)
    } else {
        req.call()
    };

    match result {
        Ok(response) => build_http_stream_response(response, None),
        Err(ureq::Error::Status(code, response)) => {
            build_http_stream_response(response, Some(code))
        }
        Err(e) => new_error(ctx.pos.clone(), format!("http.stream: {}", e)),
    }
}

pub(crate) fn build_http_response(response: ureq::Response) -> Object {
    let status = response.status();
    let status_text = response.status_text().to_string();

    let body = response.into_string().unwrap_or_default();

    ObjectBuilder::new()
        .set("status", num_obj(status as f64))
        .set("statusText", str_obj(status_text))
        .set("body", str_obj(body))
        .set("ok", bool_obj((200..300).contains(&status)))
        .build()
}

pub(crate) fn build_http_stream_response(
    response: ureq::Response,
    status_override: Option<u16>,
) -> Object {
    let status = status_override.unwrap_or_else(|| response.status());
    let status_text = response.status_text().to_string();
    let body_text = response.into_string().unwrap_or_default();
    let body = stream_from_text_object(body_text);

    ObjectBuilder::new()
        .set("status", num_obj(status as f64))
        .set("statusText", str_obj(status_text))
        .set("ok", bool_obj((200..300).contains(&status)))
        .set("body", body)
        .set(
            "close",
            native("http.stream.close", |_ctx, _args| Object::Undefined),
        )
        .build()
}

pub(crate) fn http_body_to_string(obj: &Object) -> String {
    match obj {
        Object::String(s) => s.to_string(),
        Object::Hash(h) => hash_to_json(&h.borrow()),
        Object::Array(a) => value_to_json(&Object::Array(a.clone())),
        Object::Null | Object::Undefined | Object::Boolean(_) | Object::Number(_) => {
            value_to_json(obj)
        }
        _ => value_to_string(obj),
    }
}

pub(crate) fn build_http_response_with_status(
    response: ureq::Response,
    status_code: u16,
) -> Object {
    let status_text = response.status_text().to_string();

    let body = response.into_string().unwrap_or_default();

    ObjectBuilder::new()
        .set("status", num_obj(status_code as f64))
        .set("statusText", str_obj(status_text))
        .set("body", str_obj(body))
        .set("ok", bool_obj((200..300).contains(&status_code)))
        .build()
}
