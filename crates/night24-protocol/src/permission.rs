#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    Strict,
    Permissive,
    AllowAll,
    DenyAll,
}

impl PermissionMode {
    pub const DEFAULT: Self = Self::Strict;

    pub fn normalize(mode: Option<&str>) -> Self {
        let Some(mode) = mode else {
            return Self::DEFAULT;
        };

        match mode.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "permissive" => Self::Permissive,
            "allow_all" | "full_access" => Self::AllowAll,
            "deny_all" => Self::DenyAll,
            _ => Self::Strict,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Permissive => "permissive",
            Self::AllowAll => "allow_all",
            Self::DenyAll => "deny_all",
        }
    }
}

pub fn normalize_permission_mode(mode: Option<&str>) -> String {
    PermissionMode::normalize(mode).as_str().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_supported_aliases() {
        let cases = [
            (None, PermissionMode::Strict, "strict"),
            (Some(""), PermissionMode::Strict, "strict"),
            (Some("strict"), PermissionMode::Strict, "strict"),
            (
                Some(" permissive "),
                PermissionMode::Permissive,
                "permissive",
            ),
            (Some("allow_all"), PermissionMode::AllowAll, "allow_all"),
            (Some("allow-all"), PermissionMode::AllowAll, "allow_all"),
            (Some("full_access"), PermissionMode::AllowAll, "allow_all"),
            (Some("deny_all"), PermissionMode::DenyAll, "deny_all"),
            (Some("deny-all"), PermissionMode::DenyAll, "deny_all"),
            (Some("unknown"), PermissionMode::Strict, "strict"),
        ];

        for (input, expected_mode, expected_string) in cases {
            let mode = PermissionMode::normalize(input);
            assert_eq!(mode, expected_mode);
            assert_eq!(mode.as_str(), expected_string);
            assert_eq!(normalize_permission_mode(input), expected_string);
        }
    }

    #[test]
    fn normalizes_case_and_separator_variants() {
        assert_eq!(
            PermissionMode::normalize(Some("ALLOW-ALL")),
            PermissionMode::AllowAll
        );
        assert_eq!(
            PermissionMode::normalize(Some("Deny_All")),
            PermissionMode::DenyAll
        );
        assert_eq!(
            normalize_permission_mode(Some(" Full_Access ")),
            "allow_all"
        );
    }
}
