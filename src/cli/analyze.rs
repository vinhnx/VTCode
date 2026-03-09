//! Analysis command implementations for VT Code

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum AnalysisType {
    Full,
    Structure,
    Security,
    Performance,
    Dependencies,
    Complexity,
}

impl AnalysisType {
    pub fn from_cli_arg(value: &str) -> Self {
        match value {
            "structure" => Self::Structure,
            "security" => Self::Security,
            "performance" => Self::Performance,
            "dependencies" => Self::Dependencies,
            "complexity" => Self::Complexity,
            _ => Self::Full,
        }
    }

    pub const fn default_depth(&self) -> &'static str {
        match self {
            Self::Full | Self::Structure | Self::Complexity => "deep",
            Self::Security | Self::Performance | Self::Dependencies => "standard",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AnalysisType;

    #[test]
    fn cli_arg_defaults_to_full_for_unknown_values() {
        assert!(matches!(
            AnalysisType::from_cli_arg("unknown"),
            AnalysisType::Full
        ));
    }

    #[test]
    fn deep_depth_is_used_for_structure_analysis() {
        assert_eq!(AnalysisType::Structure.default_depth(), "deep");
    }

    #[test]
    fn standard_depth_is_used_for_security_analysis() {
        assert_eq!(AnalysisType::Security.default_depth(), "standard");
    }
}
