use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    RunStarted,
    BeforeProviderRequest,
    BeforeTool,
    AfterTool,
    PermissionRequired,
    RunFinished,
    RunFailed,
}

impl HookEvent {
    pub const ALL: [Self; 7] = [
        Self::RunStarted,
        Self::BeforeProviderRequest,
        Self::BeforeTool,
        Self::AfterTool,
        Self::PermissionRequired,
        Self::RunFinished,
        Self::RunFailed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunStarted => "run_started",
            Self::BeforeProviderRequest => "before_provider_request",
            Self::BeforeTool => "before_tool",
            Self::AfterTool => "after_tool",
            Self::PermissionRequired => "permission_required",
            Self::RunFinished => "run_finished",
            Self::RunFailed => "run_failed",
        }
    }
}

impl fmt::Display for HookEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for HookEvent {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "run_started" => Ok(Self::RunStarted),
            "before_provider_request" => Ok(Self::BeforeProviderRequest),
            "before_tool" => Ok(Self::BeforeTool),
            "after_tool" => Ok(Self::AfterTool),
            "permission_required" => Ok(Self::PermissionRequired),
            "run_finished" => Ok(Self::RunFinished),
            "run_failed" => Ok(Self::RunFailed),
            _ => Err(()),
        }
    }
}

pub fn supported_hook_events() -> Vec<&'static str> {
    HookEvent::ALL.iter().map(|event| event.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_events_roundtrip_as_snake_case() {
        for event in HookEvent::ALL {
            let text = event.as_str();
            assert_eq!(text.parse::<HookEvent>(), Ok(event));
            assert_eq!(event.to_string(), text);
            assert_eq!(serde_json::to_value(event).unwrap(), text);
            assert_eq!(
                serde_json::from_str::<HookEvent>(&format!("\"{text}\"")).unwrap(),
                event
            );
        }
    }

    #[test]
    fn hook_event_parse_trims_but_rejects_unknown_values() {
        assert_eq!(
            " before_tool ".parse::<HookEvent>(),
            Ok(HookEvent::BeforeTool)
        );
        assert!("before-tool".parse::<HookEvent>().is_err());
        assert!("unknown".parse::<HookEvent>().is_err());
    }

    #[test]
    fn supported_hook_events_matches_all_variants() {
        assert_eq!(
            supported_hook_events(),
            vec![
                "run_started",
                "before_provider_request",
                "before_tool",
                "after_tool",
                "permission_required",
                "run_finished",
                "run_failed",
            ]
        );
    }
}
