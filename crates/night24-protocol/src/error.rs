use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(code: i64, message: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }

    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(PARSE_ERROR, message)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(INVALID_REQUEST, message)
    }

    pub fn method_not_found(method: impl Into<String>) -> Self {
        Self::with_data(
            METHOD_NOT_FOUND,
            "method not found",
            serde_json::json!({ "method": method.into() }),
        )
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message)
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(INTERNAL_ERROR, message)
    }

    pub fn core_not_initialized() -> Self {
        Self::new(CORE_NOT_INITIALIZED, "core not initialized")
    }

    pub fn protocol_violation(message: impl Into<String>) -> Self {
        Self::new(PROTOCOL_VIOLATION, message)
    }
}

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

pub const CORE_NOT_INITIALIZED: i64 = -32001;
pub const RUN_NOT_FOUND: i64 = -32002;
pub const RUN_ALREADY_FINISHED: i64 = -32003;
pub const PERMISSION_REQUEST_NOT_FOUND: i64 = -32004;
pub const PROVIDER_UNAVAILABLE: i64 = -32005;
pub const TOOL_EXECUTION_FAILED: i64 = -32006;
pub const CANCELLED: i64 = -32007;
pub const TIMEOUT: i64 = -32008;
pub const PROTOCOL_VIOLATION: i64 = -32009;
