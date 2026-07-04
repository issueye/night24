use chrono::Utc;
use night24_protocol::{
    AgentEvent, Capability, JsonRpcError, JsonRpcNotification, JsonRpcResponse,
};
use serde::de::DeserializeOwned;

pub(super) fn core_capabilities() -> Vec<Capability> {
    vec![
        Capability::new("core.ping", 1),
        Capability::new("core.shutdown", 1),
        Capability::new("agent.reply", 1),
        Capability::new("agent.tools", 1),
        Capability::new("agent.cancel", 1),
        Capability::new("agent.event", 1),
        Capability::new("agent.subagents", 1),
        Capability::new("agent.skills", 1),
        Capability::new("permission.resolve", 1),
    ]
}

pub(super) fn decode_params<T: DeserializeOwned>(
    params: Option<serde_json::Value>,
) -> Result<T, JsonRpcError> {
    let params = params.ok_or_else(|| JsonRpcError::invalid_params("missing params"))?;
    serde_json::from_value(params).map_err(|err| JsonRpcError::invalid_params(err.to_string()))
}

pub(super) fn decode_optional_params<T>(
    params: Option<serde_json::Value>,
) -> Result<T, JsonRpcError>
where
    T: DeserializeOwned + Default,
{
    match params {
        Some(params) => serde_json::from_value(params)
            .map_err(|err| JsonRpcError::invalid_params(err.to_string())),
        None => Ok(T::default()),
    }
}

pub(super) fn agent_event_notification(event: AgentEvent) -> String {
    serialize_notification(
        JsonRpcNotification::new("agent.event", event).unwrap_or_else(|err| JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "agent.event".to_string(),
            params: Some(serde_json::json!({
                "run_id": "run-serialization-error",
                "seq": 1,
                "type": "error",
                "created_at": Utc::now(),
                "payload": {
                    "code": "serialization_error",
                    "message": err.to_string(),
                    "recoverable": false
                }
            })),
        }),
    )
}

pub(super) fn serialize_response(response: JsonRpcResponse) -> String {
    serde_json::to_string(&response).unwrap_or_else(|err| {
        eprintln!("failed to serialize JSON-RPC response: {err}");
        r#"{"jsonrpc":"2.0","id":"rpc-serialization-error","error":{"code":-32603,"message":"serialization error"}}"#
            .to_string()
    })
}

fn serialize_notification(notification: JsonRpcNotification) -> String {
    serde_json::to_string(&notification).unwrap_or_else(|err| {
        eprintln!("failed to serialize JSON-RPC notification: {err}");
        r#"{"jsonrpc":"2.0","method":"agent.event","params":{"run_id":"run-serialization-error","seq":1,"type":"error","payload":{"code":"serialization_error","message":"serialization error","recoverable":false}}}"#.to_string()
    })
}
