//! Parser for policy rule files.
//!
//! Parses policy files in a simple line-based format similar to Codex's approach.
//! Each line specifies a command prefix and a decision.

use super::policy::{Decision, Policy, PrefixRule};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A serializable policy file format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyFile {
    /// Format version for compatibility.
    #[serde(default = "default_version")]
    pub version: u32,

    /// Policy rules.
    pub rules: Vec<PolicyRule>,
}

fn default_version() -> u32 {
    1
}

/// A single rule in the policy file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    /// Command pattern (space-separated components).
    pub pattern: String,

    /// Decision for this pattern.
    pub decision: Decision,

    /// Optional comment explaining the rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Parser for policy files.
#[derive(Debug, Default)]
pub struct PolicyParser;

impl PolicyParser {
    /// Create a new parser.
    pub fn new() -> Self {
        Self
    }

    /// Parse a TOML policy file.
    pub fn parse_toml(&self, content: &str) -> Result<PolicyFile> {
        toml::from_str(content).context("Failed to parse policy TOML")
    }

    /// Parse a JSON policy file.
    pub fn parse_json(&self, content: &str) -> Result<PolicyFile> {
        serde_json::from_str(content).context("Failed to parse policy JSON")
    }

    /// Parse a simple line-based format.
    /// Format: "decision: pattern" or "pattern = decision"
    pub fn parse_simple(&self, content: &str) -> Result<Vec<PrefixRule>> {
        let mut rules = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }

            let rule = self
                .parse_rule_line(line)
                .with_context(|| format!("Failed to parse line {}: {}", line_num + 1, line))?;

            rules.push(rule);
        }

        Ok(rules)
    }

    /// Parse a single rule line.
    fn parse_rule_line(&self, line: &str) -> Result<PrefixRule> {
        // Try "decision: pattern" format
        if let Some((decision_str, pattern)) = line.split_once(':') {
            let decision = self.parse_decision(decision_str.trim())?;
            let pattern = self.parse_pattern(pattern.trim());
            return Ok(PrefixRule::new(pattern, decision));
        }

        // Try "pattern = decision" format
        if let Some((pattern, decision_str)) = line.split_once('=') {
            let decision = self.parse_decision(decision_str.trim())?;
            let pattern = self.parse_pattern(pattern.trim());
            return Ok(PrefixRule::new(pattern, decision));
        }

        anyhow::bail!("Invalid rule format. Expected 'decision: pattern' or 'pattern = decision'")
    }

    /// Parse a decision string.
    fn parse_decision(&self, s: &str) -> Result<Decision> {
        match s.to_lowercase().as_str() {
            "allow" | "yes" | "true" | "1" => Ok(Decision::Allow),
            "prompt" | "ask" | "confirm" => Ok(Decision::Prompt),
            "forbidden" | "forbid" | "deny" | "no" | "false" | "0" => Ok(Decision::Forbidden),
            _ => anyhow::bail!("Invalid decision: {}", s),
        }
    }

    /// Parse a pattern string into components.
    fn parse_pattern(&self, s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    /// Load a policy from a file.
    pub async fn load_file(&self, path: &Path) -> Result<Policy> {
        let content = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read policy file: {}", path.display()))?;

        self.load_from_content(&content, path)
    }

    /// Load a policy from file content.
    pub fn load_from_content(&self, content: &str, path: &Path) -> Result<Policy> {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let rules = match extension {
            "toml" => {
                let file = self.parse_toml(content)?;
                file.rules
                    .into_iter()
                    .map(|r| {
                        PrefixRule::new(
                            r.pattern.split_whitespace().map(String::from).collect(),
                            r.decision,
                        )
                    })
                    .collect()
            }
            "json" => {
                let file = self.parse_json(content)?;
                file.rules
                    .into_iter()
                    .map(|r| {
                        PrefixRule::new(
                            r.pattern.split_whitespace().map(String::from).collect(),
                            r.decision,
                        )
                    })
                    .collect()
            }
            _ => self.parse_simple(content)?,
        };

        let mut policy = Policy::empty();
        for rule in rules {
            policy.add_prefix_rule(&rule.pattern, rule.decision)?;
        }

        Ok(policy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_format() {
        let parser = PolicyParser::new();
        let content = r#"
# Allow cargo commands
allow: cargo build
allow: cargo test

# Forbid dangerous commands
forbidden: rm -rf
prompt: git push
"#;

        let rules = parser.parse_simple(content).unwrap();
        assert_eq!(rules.len(), 4);

        assert_eq!(rules[0].pattern, vec!["cargo", "build"]);
        assert_eq!(rules[0].decision, Decision::Allow);

        assert_eq!(rules[3].pattern, vec!["git", "push"]);
        assert_eq!(rules[3].decision, Decision::Prompt);
    }

    #[test]
    fn test_parse_equals_format() {
        let parser = PolicyParser::new();
        let content = r#"
cargo build = allow
rm -rf = deny
"#;

        let rules = parser.parse_simple(content).unwrap();
        assert_eq!(rules.len(), 2);

        assert_eq!(rules[0].decision, Decision::Allow);
        assert_eq!(rules[1].decision, Decision::Forbidden);
    }

    #[test]
    fn test_parse_toml() {
        let parser = PolicyParser::new();
        let content = r#"
version = 1

[[rules]]
pattern = "cargo build"
decision = "allow"

[[rules]]
pattern = "rm -rf"
decision = "forbidden"
comment = "Never allow recursive delete"
"#;

        let file = parser.parse_toml(content).unwrap();
        assert_eq!(file.rules.len(), 2);
        assert_eq!(file.rules[0].decision, Decision::Allow);
        assert_eq!(
            file.rules[1].comment,
            Some("Never allow recursive delete".to_string())
        );
    }

    #[test]
    fn test_parse_json() {
        let parser = PolicyParser::new();
        let content = r#"{
			"version": 1,
			"rules": [
				{"pattern": "cargo test", "decision": "allow"},
				{"pattern": "git push", "decision": "prompt"}
			]
		}"#;

        let file = parser.parse_json(content).unwrap();
        assert_eq!(file.rules.len(), 2);
    }
}
