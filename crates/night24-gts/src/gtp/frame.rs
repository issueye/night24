//! GTP Frame and Value structures
//!
//! This module defines the core data structures for the GTP (GoScript Transport Protocol).
//! It matches the Go version's structure for compatibility.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// GTP protocol version
pub const VERSION: u32 = 1;

/// A GTP frame - the basic unit of communication
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Frame {
    /// Protocol version (currently 1)
    #[serde(rename = "v")]
    pub version: u32,

    /// Frame ID (unique identifier for request/response matching)
    pub id: String,

    /// Frame type: "hello", "ready", "call", "result", "event", "cancel"
    #[serde(rename = "type")]
    pub frame_type: String,

    // Handshake fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,

    /// Modules exposed by the plugin (for ready frame)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modules: Option<serde_json::Value>,

    // Call/Result fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<Value>>,

    #[serde(rename = "deadlineMs", skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<i64>,

    // Result fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<GtpError>,

    // Event fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,

    // Cancel fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// GTP Value - cross-process value encoding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Value {
    /// Type tag: "undefined", "null", "boolean", "number", "string", "bytes", "array", "object", "resource", "error"
    #[serde(rename = "$t")]
    pub value_type: String,

    /// The actual value (for primitive types, stored in serde_json::Value)
    #[serde(rename = "v", skip_serializing_if = "Option::is_none")]
    pub v: Option<serde_json::Value>,

    /// Encoding for bytes type (e.g., "base64")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,

    /// Special values: "NaN", "Infinity", "-Infinity"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub special: Option<String>,

    // Resource fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub methods: Option<Vec<String>>,

    // Error fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// GTP Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GtpError {
    /// Error name (e.g., "TypeError", "HostError")
    pub name: String,

    /// Error message
    pub message: String,

    /// Error code (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Additional details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<HashMap<String, serde_json::Value>>,
}

// ============================================================================
// Helper constructors for Value
// ============================================================================

impl Value {
    /// Create an undefined value
    pub fn undefined() -> Self {
        Value {
            value_type: "undefined".to_string(),
            v: None,
            encoding: None,
            special: None,
            id: None,
            kind: None,
            methods: None,
            name: None,
            message: None,
        }
    }

    /// Create a null value
    pub fn null() -> Self {
        Value {
            value_type: "null".to_string(),
            v: None,
            encoding: None,
            special: None,
            id: None,
            kind: None,
            methods: None,
            name: None,
            message: None,
        }
    }

    /// Create a boolean value
    pub fn boolean(b: bool) -> Self {
        Value {
            value_type: "boolean".to_string(),
            v: Some(serde_json::Value::Bool(b)),
            encoding: None,
            special: None,
            id: None,
            kind: None,
            methods: None,
            name: None,
            message: None,
        }
    }

    /// Create a number value
    pub fn number(n: f64) -> Self {
        if n.is_nan() {
            return Value {
                value_type: "number".to_string(),
                v: None,
                encoding: None,
                special: Some("NaN".to_string()),
                id: None,
                kind: None,
                methods: None,
                name: None,
                message: None,
            };
        }
        if n.is_infinite() {
            let special = if n.is_sign_positive() {
                "Infinity"
            } else {
                "-Infinity"
            };
            return Value {
                value_type: "number".to_string(),
                v: None,
                encoding: None,
                special: Some(special.to_string()),
                id: None,
                kind: None,
                methods: None,
                name: None,
                message: None,
            };
        }
        Value {
            value_type: "number".to_string(),
            v: Some(serde_json::json!(n)),
            encoding: None,
            special: None,
            id: None,
            kind: None,
            methods: None,
            name: None,
            message: None,
        }
    }

    /// Create a string value
    pub fn string(s: String) -> Self {
        Value {
            value_type: "string".to_string(),
            v: Some(serde_json::Value::String(s)),
            encoding: None,
            special: None,
            id: None,
            kind: None,
            methods: None,
            name: None,
            message: None,
        }
    }

    /// Create a bytes value (base64 encoded)
    pub fn bytes(data: Vec<u8>) -> Self {
        use base64::{engine::general_purpose, Engine as _};
        let encoded = general_purpose::STANDARD.encode(&data);
        Value {
            value_type: "bytes".to_string(),
            v: Some(serde_json::Value::String(encoded)),
            encoding: Some("base64".to_string()),
            special: None,
            id: None,
            kind: None,
            methods: None,
            name: None,
            message: None,
        }
    }

    /// Create an array value
    pub fn array(items: Vec<Value>) -> Self {
        Value {
            value_type: "array".to_string(),
            v: Some(serde_json::to_value(items).unwrap()),
            encoding: None,
            special: None,
            id: None,
            kind: None,
            methods: None,
            name: None,
            message: None,
        }
    }

    /// Create an object value
    pub fn object(fields: HashMap<String, Value>) -> Self {
        Value {
            value_type: "object".to_string(),
            v: Some(serde_json::to_value(fields).unwrap()),
            encoding: None,
            special: None,
            id: None,
            kind: None,
            methods: None,
            name: None,
            message: None,
        }
    }

    /// Create a resource value
    pub fn resource(id: String, kind: String, methods: Vec<String>) -> Self {
        Value {
            value_type: "resource".to_string(),
            v: None,
            encoding: None,
            special: None,
            id: Some(id),
            kind: Some(kind),
            methods: Some(methods),
            name: None,
            message: None,
        }
    }

    /// Create an error value
    pub fn error(name: String, message: String) -> Self {
        Value {
            value_type: "error".to_string(),
            v: None,
            encoding: None,
            special: None,
            id: None,
            kind: None,
            methods: None,
            name: Some(name),
            message: Some(message),
        }
    }
}

// ============================================================================
// Helper constructors for GtpError
// ============================================================================

impl GtpError {
    /// Create a TypeError
    pub fn type_error(message: String) -> Self {
        GtpError {
            name: "TypeError".to_string(),
            message,
            code: None,
            details: None,
        }
    }

    /// Create a HostError
    pub fn host_error(message: String) -> Self {
        GtpError {
            name: "HostError".to_string(),
            message,
            code: None,
            details: None,
        }
    }

    /// Create a NotFoundError
    pub fn not_found_error(message: String) -> Self {
        GtpError {
            name: "NotFoundError".to_string(),
            message,
            code: None,
            details: None,
        }
    }
}

// ============================================================================
// Helper constructors for Frame
// ============================================================================

impl Frame {
    /// Create a hello frame
    pub fn hello(id: String, runtime: Option<String>) -> Self {
        Frame {
            version: VERSION,
            id,
            frame_type: "hello".to_string(),
            runtime,
            ..Default::default()
        }
    }

    /// Create a ready frame
    pub fn ready(
        id: String,
        service: Option<String>,
        capabilities: Vec<String>,
        modules: Option<serde_json::Value>,
    ) -> Self {
        Frame {
            version: VERSION,
            id,
            frame_type: "ready".to_string(),
            service,
            capabilities: Some(capabilities),
            modules,
            ..Default::default()
        }
    }

    /// Create a call frame
    pub fn call(id: String, module: String, method: String, args: Vec<Value>) -> Self {
        Frame {
            version: VERSION,
            id,
            frame_type: "call".to_string(),
            module: Some(module),
            method: Some(method),
            args: Some(args),
            ..Default::default()
        }
    }

    /// Create an OK result frame
    pub fn ok_result(id: String, result: Value) -> Self {
        Frame {
            version: VERSION,
            id,
            frame_type: "result".to_string(),
            ok: Some(true),
            result: Some(result),
            ..Default::default()
        }
    }

    /// Create an error result frame
    pub fn error_result(id: String, error: GtpError) -> Self {
        Frame {
            version: VERSION,
            id,
            frame_type: "result".to_string(),
            ok: Some(false),
            error: Some(error),
            ..Default::default()
        }
    }

    /// Create an event frame
    pub fn event(id: String, module: String, event: String, data: Value) -> Self {
        Frame {
            version: VERSION,
            id,
            frame_type: "event".to_string(),
            module: Some(module),
            event: Some(event),
            data: Some(data),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_constructors() {
        let v = Value::undefined();
        assert_eq!(v.value_type, "undefined");

        let v = Value::null();
        assert_eq!(v.value_type, "null");

        let v = Value::boolean(true);
        assert_eq!(v.value_type, "boolean");
        assert_eq!(v.v, Some(serde_json::Value::Bool(true)));

        let v = Value::number(42.0);
        assert_eq!(v.value_type, "number");

        let v = Value::number(f64::NAN);
        assert_eq!(v.value_type, "number");
        assert_eq!(v.special, Some("NaN".to_string()));

        let v = Value::string("hello".to_string());
        assert_eq!(v.value_type, "string");
    }

    #[test]
    fn test_frame_constructors() {
        let f = Frame::hello("h1".to_string(), Some("gts_r".to_string()));
        assert_eq!(f.frame_type, "hello");
        assert_eq!(f.id, "h1");
        assert_eq!(f.runtime, Some("gts_r".to_string()));

        let f = Frame::call(
            "c1".to_string(),
            "@plugin/test".to_string(),
            "doSomething".to_string(),
            vec![Value::number(42.0)],
        );
        assert_eq!(f.frame_type, "call");
        assert_eq!(f.module, Some("@plugin/test".to_string()));
        assert_eq!(f.method, Some("doSomething".to_string()));
    }

    #[test]
    fn test_serialization() {
        let f = Frame::hello("h1".to_string(), Some("gts_r".to_string()));
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"type\":\"hello\""));
        assert!(json.contains("\"v\":1"));

        let f2: Frame = serde_json::from_str(&json).unwrap();
        assert_eq!(f2.frame_type, "hello");
        assert_eq!(f2.version, 1);
    }
}
