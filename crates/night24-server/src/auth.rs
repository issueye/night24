use axum::{
    extract::{Request, State},
    http::header,
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

    let headers = request.headers();
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.trim().to_string())
        });

    match provided {
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
}
