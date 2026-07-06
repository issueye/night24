use axum::{
    extract::{Request, State},
    http::{header, HeaderMap},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

/// Whether a request path is exempt from authentication (health/docs).
pub(crate) fn is_public_path(path: &str) -> bool {
    path == "/healthz"
        || path == "/readyz"
        || path.starts_with("/swagger-ui")
        || path.starts_with("/api-docs")
}

/// Middleware that enforces an API key when `NIGHT24_API_KEY` is configured.
///
/// Accepted formats:
/// - `Authorization: Bearer <key>`
/// - `X-API-Key: <key>`
///
/// Public, unauthenticated paths: `/healthz`, `/swagger-ui*`, `/api-docs*`.
pub(crate) async fn require_api_key(
    State(expected_key): State<String>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if is_public_path(path) {
        return next.run(request).await;
    }

    match provided_api_key(request.headers()) {
        Some(key) if constant_time_eq(key.as_bytes(), expected_key.as_bytes()) => {
            next.run(request).await
        }
        _ => (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "missing or invalid api key"})),
        )
            .into_response(),
    }
}

pub(crate) fn provided_api_key(headers: &HeaderMap) -> Option<&str> {
    header_str(headers, header::AUTHORIZATION)
        .and_then(bearer_token)
        .or_else(|| header_str(headers, "x-api-key").map(str::trim))
}

fn header_str<'a>(
    headers: &'a HeaderMap,
    name: impl axum::http::header::AsHeaderName,
) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn bearer_token(value: &str) -> Option<&str> {
    value.strip_prefix("Bearer ").map(str::trim)
}

/// Compare two byte slices in constant time to avoid timing side channels.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn constant_time_eq_matching() {
        assert!(constant_time_eq(b"secret", b"secret"));
    }

    #[test]
    fn constant_time_eq_different() {
        assert!(!constant_time_eq(b"secret", b"secre7"));
        assert!(!constant_time_eq(b"secret", b"secret2"));
        assert!(!constant_time_eq(b"short", b"longer"));
    }

    #[test]
    fn public_path_detection() {
        assert!(is_public_path("/healthz"));
        assert!(is_public_path("/readyz"));
        assert!(is_public_path("/swagger-ui"));
        assert!(is_public_path("/swagger-ui/"));
        assert!(is_public_path("/api-docs/openapi.json"));
        assert!(!is_public_path("/reply"));
        assert!(!is_public_path("/sessions"));
        assert!(!is_public_path("/sessions/123/history"));
    }

    #[test]
    fn provided_api_key_prefers_bearer_authorization() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer  secret-from-auth  "),
        );
        headers.insert("x-api-key", HeaderValue::from_static("secret-from-api-key"));

        assert_eq!(provided_api_key(&headers), Some("secret-from-auth"));
    }

    #[test]
    fn provided_api_key_falls_back_to_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_static("  secret-from-api-key  "),
        );

        assert_eq!(provided_api_key(&headers), Some("secret-from-api-key"));
    }

    #[test]
    fn provided_api_key_rejects_non_bearer_authorization() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic secret"),
        );

        assert_eq!(provided_api_key(&headers), None);
    }

    #[test]
    fn provided_api_key_falls_back_when_authorization_is_not_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic secret"),
        );
        headers.insert("x-api-key", HeaderValue::from_static("secret-from-api-key"));

        assert_eq!(provided_api_key(&headers), Some("secret-from-api-key"));
    }

    #[test]
    fn provided_api_key_falls_back_when_authorization_is_not_utf8() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_bytes(b"Bearer \xFF").unwrap(),
        );
        headers.insert("x-api-key", HeaderValue::from_static("secret-from-api-key"));

        assert_eq!(provided_api_key(&headers), Some("secret-from-api-key"));
    }
}
