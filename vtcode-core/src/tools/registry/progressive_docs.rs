//! Progressive tool documentation loading system
//!
//! Inspired by pi-coding-agent's philosophy: load minimal tool signatures upfront,
//! detailed documentation on-demand. This reduces initial token overhead by 60-73%.
//!
//! # Three-Tier Documentation Model
//!
//! 1. **Minimal Signature** (~40 tokens/tool) - Always loaded
//!    - Tool name + one-line description
//!    - Required parameters only
//!
//! 2. **Standard Documentation** (~80 tokens/tool) - On first use or error
//!    - Brief but complete description
//!    - All parameters with short descriptions
//!
//! 3. **Full Documentation** (~225 tokens/tool) - On explicit request
//!    - Comprehensive description with examples
//!    - All parameters with detailed descriptions
//!    - Usage examples and edge cases
//!
//! # Token Savings
//!
//! Current:    ~3,000 tokens (22 tools × ~135 avg)
//! Progressive: ~1,200 tokens (22 tools × ~55 avg)
//! Minimal:      ~800 tokens (22 tools × ~36 avg)
//!
//! Savings: 60-73% reduction in tool documentation overhead

use crate::config::constants::tools;
use crate::gemini::FunctionDeclaration;
use serde::{Deserialize, Serialize};
use serde_json::{Map, json};
use std::collections::HashMap;

/// Documentation mode for tools
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ToolDocumentationMode {
    /// Minimal signatures only (~800 tokens total)
    /// Best for: Power users, token-constrained contexts
    Minimal,

    /// Signatures + smart hints (~1,200 tokens total)
    /// Best for: General usage, balances overhead vs guidance
    #[default]
    Progressive,

    /// Full documentation upfront (~3,000 tokens total)
    /// Best for: Maximum hand-holding, current behavior
    Full,
}

impl ToolDocumentationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Progressive => "progressive",
            Self::Full => "full",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "minimal" => Some(Self::Minimal),
            "progressive" => Some(Self::Progressive),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
}

/// Minimal tool signature for progressive loading
#[derive(Debug, Clone)]
pub struct ToolSignature {
    /// Tool name
    pub name: &'static str,

    /// Brief one-line description (15-30 chars)
    pub brief: &'static str,

    /// Required parameters with minimal descriptions
    pub required_params: Vec<(&'static str, &'static str, &'static str)>, // (name, type, brief)

    /// Optional common parameters (shown in progressive mode)
    pub common_params: Vec<(&'static str, &'static str, &'static str)>,

    /// Estimated token count for this signature
    pub token_estimate: u32,
}

/// Minimal tool signatures for all built-in tools
pub fn minimal_tool_signatures() -> HashMap<&'static str, ToolSignature> {
    let mut sigs = HashMap::new();

    // SEARCH & DISCOVERY
    sigs.insert(
        tools::GREP_FILE,
        ToolSignature {
            name: tools::GREP_FILE,
            brief: "Search code with regex",
            required_params: vec![("pattern", "string", "Search pattern")],
            common_params: vec![
                ("path", "string", "Directory"),
                ("max_results", "integer", "Result limit"),
                ("literal", "boolean", "Exact match"),
            ],
            token_estimate: 40,
        },
    );

    sigs.insert(
        tools::LIST_FILES,
        ToolSignature {
            name: tools::LIST_FILES,
            brief: "Explore directories",
            required_params: vec![("path", "string", "Directory path")],
            common_params: vec![
                ("mode", "string", "list|recursive|find"),
                ("max_items", "integer", "Scan limit"),
            ],
            token_estimate: 35,
        },
    );

    // EXECUTION
    sigs.insert(
        tools::RUN_PTY_CMD,
        ToolSignature {
            name: tools::RUN_PTY_CMD,
            brief: "Execute shell commands",
            required_params: vec![("command", "string", "Shell command")],
            common_params: vec![
                ("timeout", "integer", "Max seconds"),
                ("env", "object", "Environment vars"),
            ],
            token_estimate: 35,
        },
    );

    sigs.insert(
        tools::UNIFIED_EXEC,
        ToolSignature {
            name: tools::UNIFIED_EXEC,
            brief: "Run or inspect command sessions",
            required_params: vec![],
            common_params: vec![
                (
                    "action",
                    "string",
                    "run|write|poll|continue|inspect|list|close|code",
                ),
                ("command", "string", "Command for run"),
                (
                    "session_id",
                    "string",
                    "Session for write/poll/continue/inspect/close",
                ),
            ],
            token_estimate: 40,
        },
    );

    // FILE OPERATIONS
    sigs.insert(
        tools::READ_FILE,
        ToolSignature {
            name: tools::READ_FILE,
            brief: "Read file contents",
            required_params: vec![("path", "string", "File path")],
            common_params: vec![
                ("offset", "integer", "Start line"),
                ("limit", "integer", "Line count"),
                ("max_tokens", "integer", "Token limit"),
            ],
            token_estimate: 40,
        },
    );

    sigs.insert(
        tools::UNIFIED_FILE,
        ToolSignature {
            name: tools::UNIFIED_FILE,
            brief: "Read/write/edit/patch files",
            required_params: vec![("path", "string", "File path")],
            common_params: vec![
                ("action", "string", "read|write|edit|patch|delete|move|copy"),
                ("content", "string", "Content for write"),
                ("old_str", "string", "Match text for edit"),
                ("new_str", "string", "Replacement for edit"),
            ],
            token_estimate: 42,
        },
    );

    sigs.insert(
        tools::UNIFIED_SEARCH,
        ToolSignature {
            name: tools::UNIFIED_SEARCH,
            brief: "Search files, tools, agent state, web, skills",
            required_params: vec![("action", "string", "grep|list|tools|errors|agent|web|skill")],
            common_params: vec![
                ("pattern", "string", "Pattern for grep/errors"),
                ("path", "string", "Target directory"),
                ("keyword", "string", "Keyword for tools action"),
            ],
            token_estimate: 42,
        },
    );

    // NOTE: create_file removed - use write_file with mode=fail_if_exists

    sigs.insert(
        tools::DELETE_FILE,
        ToolSignature {
            name: tools::DELETE_FILE,
            brief: "Delete file",
            required_params: vec![("path", "string", "File path")],
            common_params: vec![],
            token_estimate: 25,
        },
    );

    sigs.insert(
        tools::WRITE_FILE,
        ToolSignature {
            name: tools::WRITE_FILE,
            brief: "Write/overwrite file",
            required_params: vec![
                ("path", "string", "File path"),
                ("content", "string", "File content"),
            ],
            common_params: vec![],
            token_estimate: 30,
        },
    );

    sigs.insert(
        tools::EDIT_FILE,
        ToolSignature {
            name: tools::EDIT_FILE,
            brief: "Edit file precisely",
            required_params: vec![
                ("path", "string", "File path"),
                ("old_str", "string", "Text to replace"),
                ("new_str", "string", "Replacement text"),
            ],
            common_params: vec![],
            token_estimate: 45,
        },
    );

    sigs.insert(
        tools::APPLY_PATCH,
        ToolSignature {
            name: tools::APPLY_PATCH,
            brief: "Apply VT Code patch (*** Begin/End Patch format)",
            required_params: vec![("input", "string", "VT Code patch content")],
            common_params: vec![("patch", "string", "Alias for input")],
            token_estimate: 35,
        },
    );

    // NOTE: search_replace removed - use edit_file instead

    // TOOLS & SKILLS
    sigs.insert(
        tools::SEARCH_TOOLS,
        ToolSignature {
            name: tools::SEARCH_TOOLS,
            brief: "Find tools by keyword",
            required_params: vec![("keyword", "string", "Search term")],
            common_params: vec![],
            token_estimate: 30,
        },
    );

    sigs.insert(
        tools::TASK_TRACKER,
        ToolSignature {
            name: tools::TASK_TRACKER,
            brief: "Track multi-step checklist",
            required_params: vec![("action", "string", "create|update|list|add")],
            common_params: vec![
                ("items", "array", "Task descriptions"),
                ("index", "integer", "Item index for update"),
                ("status", "string", "pending|in_progress|completed|blocked"),
            ],
            token_estimate: 35,
        },
    );

    sigs.insert(
        tools::PLAN_TASK_TRACKER,
        ToolSignature {
            name: tools::PLAN_TASK_TRACKER,
            brief: "Plan-mode scoped checklist",
            required_params: vec![("action", "string", "create|update|list|add")],
            common_params: vec![
                ("items", "array", "Task descriptions"),
                ("index_path", "string", "Hierarchical path (e.g., 2.1)"),
                ("status", "string", "pending|in_progress|completed|blocked"),
            ],
            token_estimate: 35,
        },
    );

    sigs.insert(
        tools::SKILL,
        ToolSignature {
            name: tools::SKILL,
            brief: "Load pre-built skill",
            required_params: vec![("name", "string", "Skill name")],
            common_params: vec![],
            token_estimate: 30,
        },
    );

    // Merged agent diagnostics tool
    sigs.insert(
        tools::AGENT_INFO,
        ToolSignature {
            name: tools::AGENT_INFO,
            brief: "Agent diagnostics",
            required_params: vec![],
            common_params: vec![("mode", "string", "debug|analyze|full")],
            token_estimate: 30,
        },
    );

    // EXECUTION
    sigs.insert(
        tools::EXECUTE_CODE,
        ToolSignature {
            name: tools::EXECUTE_CODE,
            brief: "Execute code snippet",
            required_params: vec![
                ("code", "string", "Code to execute"),
                ("language", "string", "Language"),
            ],
            common_params: vec![("timeout", "integer", "Max seconds")],
            token_estimate: 40,
        },
    );

    // NOTE: PTY session tools hidden from LLM - use run_pty_cmd

    // WEB
    sigs.insert(
        "web_fetch",
        ToolSignature {
            name: "web_fetch",
            brief: "Fetch web content",
            required_params: vec![("url", "string", "URL to fetch")],
            common_params: vec![("timeout", "integer", "Max seconds")],
            token_estimate: 35,
        },
    );

    sigs
}

/// Build minimal function declarations
pub fn build_minimal_declarations(
    signatures: &HashMap<&str, ToolSignature>,
) -> Vec<FunctionDeclaration> {
    signatures
        .values()
        .map(|sig| {
            let mut properties = Map::new();

            // Add required parameters
            for (name, typ, desc) in &sig.required_params {
                properties.insert(
                    name.to_string(),
                    json!({
                        "type": typ,
                        "description": desc
                    }),
                );
            }

            let required: Vec<String> = sig
                .required_params
                .iter()
                .map(|(n, _, _)| n.to_string())
                .collect();

            FunctionDeclaration {
                name: sig.name.to_owned(),
                description: sig.brief.to_owned(),
                parameters: json!({
                    "type": "object",
                    "properties": properties,
                    "required": required
                }),
            }
        })
        .collect()
}

/// Build progressive function declarations (signatures + common params)
pub fn build_progressive_declarations(
    signatures: &HashMap<&str, ToolSignature>,
) -> Vec<FunctionDeclaration> {
    signatures
        .values()
        .map(|sig| {
            let mut properties = Map::new();

            // Add required parameters
            for (name, typ, desc) in &sig.required_params {
                properties.insert(
                    name.to_string(),
                    json!({
                        "type": typ,
                        "description": desc
                    }),
                );
            }

            // Add common parameters (optional)
            for (name, typ, desc) in &sig.common_params {
                properties.insert(
                    name.to_string(),
                    json!({
                        "type": typ,
                        "description": desc
                    }),
                );
            }

            let required: Vec<String> = sig
                .required_params
                .iter()
                .map(|(n, _, _)| n.to_string())
                .collect();

            FunctionDeclaration {
                name: sig.name.to_owned(),
                description: format!("{}. Use for common operations.", sig.brief),
                parameters: json!({
                    "type": "object",
                    "properties": properties,
                    "required": required
                }),
            }
        })
        .collect()
}

/// Calculate total token estimate for a mode
pub fn estimate_tokens(mode: ToolDocumentationMode, num_tools: usize) -> u32 {
    match mode {
        ToolDocumentationMode::Minimal => (num_tools as u32) * 36, // ~800 for 22 tools
        ToolDocumentationMode::Progressive => (num_tools as u32) * 55, // ~1,200 for 22 tools
        ToolDocumentationMode::Full => (num_tools as u32) * 135,   // ~3,000 for 22 tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimal_signatures_coverage() {
        let sigs = minimal_tool_signatures();

        // Verify we have signatures for key tools
        assert!(sigs.contains_key(tools::GREP_FILE));
        assert!(sigs.contains_key(tools::LIST_FILES));
        assert!(sigs.contains_key(tools::RUN_PTY_CMD));
        assert!(sigs.contains_key(tools::READ_FILE));
        assert!(sigs.contains_key(tools::EDIT_FILE));

        // Should have at least 20 tools
        assert!(
            sigs.len() >= 20,
            "Expected >= 20 tool signatures, got {}",
            sigs.len()
        );
    }

    #[test]
    fn test_token_estimates() {
        let sigs = minimal_tool_signatures();

        // Each signature should have reasonable token estimate
        for sig in sigs.values() {
            assert!(
                sig.token_estimate >= 20 && sig.token_estimate <= 60,
                "Token estimate for {} out of range: {}",
                sig.name,
                sig.token_estimate
            );
        }

        // Total should be reasonable
        let total: u32 = sigs.values().map(|s| s.token_estimate).sum();
        assert!(
            total >= 600 && total <= 1200,
            "Total token estimate out of range: {}",
            total
        );
    }

    #[test]
    fn test_build_minimal_declarations() {
        let sigs = minimal_tool_signatures();
        let decls = build_minimal_declarations(&sigs);

        // Should have same number of declarations as signatures
        assert_eq!(decls.len(), sigs.len());

        // Each declaration should have minimal content
        for decl in &decls {
            assert!(
                decl.description.len() < 50,
                "Description too long: {}",
                decl.description
            );
        }
    }

    #[test]
    fn test_build_progressive_declarations() {
        let sigs = minimal_tool_signatures();
        let decls = build_progressive_declarations(&sigs);

        // Should have same number of declarations
        assert_eq!(decls.len(), sigs.len());

        // Descriptions should be slightly longer than minimal
        for decl in &decls {
            assert!(
                decl.description.len() >= 15,
                "Description too short: {}",
                decl.description
            );
        }
    }

    #[test]
    fn test_mode_parsing() {
        assert_eq!(
            ToolDocumentationMode::parse("minimal"),
            Some(ToolDocumentationMode::Minimal)
        );
        assert_eq!(
            ToolDocumentationMode::parse("PROGRESSIVE"),
            Some(ToolDocumentationMode::Progressive)
        );
        assert_eq!(
            ToolDocumentationMode::parse("Full"),
            Some(ToolDocumentationMode::Full)
        );
        assert_eq!(ToolDocumentationMode::parse("invalid"), None);
    }

    #[test]
    fn test_token_estimation() {
        assert_eq!(estimate_tokens(ToolDocumentationMode::Minimal, 22), 22 * 36);
        assert_eq!(
            estimate_tokens(ToolDocumentationMode::Progressive, 22),
            22 * 55
        );
        assert_eq!(estimate_tokens(ToolDocumentationMode::Full, 22), 22 * 135);
    }

    #[test]
    fn test_integration_with_declarations() {
        // Verify that minimal and progressive modes produce valid declarations
        let sigs = minimal_tool_signatures();

        let minimal_decls = build_minimal_declarations(&sigs);
        let progressive_decls = build_progressive_declarations(&sigs);

        // Should have same number of declarations as signatures
        assert_eq!(minimal_decls.len(), sigs.len());
        assert_eq!(progressive_decls.len(), sigs.len());

        // Progressive should have more parameters than minimal
        for (minimal, progressive) in minimal_decls.iter().zip(progressive_decls.iter()) {
            assert_eq!(minimal.name, progressive.name, "Names should match");

            // Progressive should have more description
            assert!(
                progressive.description.len() >= minimal.description.len(),
                "Progressive description should be >= minimal for {}",
                minimal.name
            );
        }
    }

    #[test]
    fn test_mode_default() {
        // Verify default mode is Progressive (token-efficient default)
        assert_eq!(
            ToolDocumentationMode::default(),
            ToolDocumentationMode::Progressive
        );
    }
}
