use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use night24_core::model::{Message, Tool};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub version: u32,
}

impl Capability {
    pub fn new(name: impl Into<String>, version: u32) -> Self {
        Self {
            name: name.into(),
            version,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub name: String,
    pub version: String,
}

impl PeerInfo {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InitializeEnvironment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    pub protocol_version: String,
    pub client: PeerInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<PathBuf>,
    #[serde(default)]
    pub environment: InitializeEnvironment,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    pub protocol_version: String,
    pub server: PeerInfo,
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PingParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShutdownParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grace_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptedResult {
    pub accepted: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentToolsParams {
    #[serde(default)]
    pub include_disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolsResult {
    pub tools: Vec<Tool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplySession {
    pub id: String,
    pub name: String,
    pub working_dir: PathBuf,
    #[serde(default)]
    pub conversation: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyInput {
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyLimits {
    pub max_turns: usize,
    pub turn_timeout_ms: u64,
    pub tool_timeout_ms: u64,
    pub total_timeout_ms: u64,
}

impl Default for ReplyLimits {
    fn default() -> Self {
        Self {
            max_turns: 40,
            turn_timeout_ms: 60_000,
            tool_timeout_ms: 60_000,
            total_timeout_ms: 600_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyOptions {
    #[serde(default)]
    pub stream_message_delta: bool,
    #[serde(default = "default_true")]
    pub emit_tool_events: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
}

impl Default for ReplyOptions {
    fn default() -> Self {
        Self {
            stream_message_delta: false,
            emit_tool_events: true,
            permission_mode: None,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyParams {
    pub run_id: String,
    pub session: ReplySession,
    pub input: ReplyInput,
    #[serde(default)]
    pub provider: ProviderConfig,
    #[serde(default)]
    pub limits: ReplyLimits,
    #[serde(default)]
    pub options: ReplyOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyAccepted {
    pub accepted: bool,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelParams {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Approve,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResolution {
    pub run_id: String,
    pub permission_id: String,
    pub decision: PermissionDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MethodParams {
    #[serde(default)]
    pub extra: BTreeMap<String, serde_json::Value>,
}
