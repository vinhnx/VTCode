//! Risk scoring system for tool execution

use serde::{Deserialize, Serialize};

/// Risk level classification for tools
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskLevel {
    /// Read-only operations with no side effects
    Low,

    /// Operations that create/modify data but within trusted boundaries
    Medium,

    /// Operations with potentially destructive effects or external access
    High,

    /// Operations that could compromise system security
    Critical,
}

impl RiskLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub fn color_code(self) -> &'static str {
        match self {
            Self::Low => "\x1b[32m",      // green
            Self::Medium => "\x1b[33m",   // yellow
            Self::High => "\x1b[31m",     // red
            Self::Critical => "\x1b[35m", // magenta
        }
    }
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source of the tool (internal, MCP, ACP, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSource {
    /// Built-in tools
    Internal,

    /// Model Context Protocol (external)
    Mcp,

    /// Agent Client Protocol (IDE integration)
    Acp,

    /// Other external sources
    External,
}

impl ToolSource {
    /// Get the risk multiplier for this source
    /// MCP/external tools are considered higher risk
    pub fn risk_multiplier(self) -> f32 {
        match self {
            Self::Internal => 1.0,
            Self::Mcp => 1.5,
            Self::Acp => 1.2,
            Self::External => 2.0,
        }
    }
}

/// Workspace trust level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkspaceTrust {
    Untrusted,
    Partial,
    Trusted,
    FullAuto,
}

impl WorkspaceTrust {
    /// Get the risk reduction multiplier for trusted workspaces
    pub fn risk_reduction(self) -> f32 {
        match self {
            Self::Untrusted => 1.0,
            Self::Partial => 0.8,
            Self::Trusted => 0.6,
            Self::FullAuto => 0.3,
        }
    }
}

/// Context for risk assessment
#[derive(Debug, Clone)]
pub struct ToolRiskContext {
    /// Tool name
    pub tool_name: String,

    /// Source of the tool
    pub source: ToolSource,

    /// Workspace trust level
    pub workspace_trust: WorkspaceTrust,

    /// Number of times this tool has been approved recently
    pub recent_approvals: usize,

    /// Command arguments (if applicable)
    pub command_args: Vec<String>,

    /// Whether this is a write operation
    pub is_write: bool,

    /// Whether this is a potentially destructive operation
    pub is_destructive: bool,

    /// Whether this accesses external network
    pub accesses_network: bool,
}

impl ToolRiskContext {
    /// Create a new risk context
    pub fn new(tool_name: String, source: ToolSource, workspace_trust: WorkspaceTrust) -> Self {
        Self {
            tool_name,
            source,
            workspace_trust,
            recent_approvals: 0,
            command_args: Vec::new(),
            is_write: false,
            is_destructive: false,
            accesses_network: false,
        }
    }

    /// Set command arguments
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.command_args = args;
        self
    }

    /// Mark as write operation
    pub fn as_write(mut self) -> Self {
        self.is_write = true;
        self
    }

    /// Mark as potentially destructive
    pub fn as_destructive(mut self) -> Self {
        self.is_destructive = true;
        self
    }

    /// Mark as network-accessing
    pub fn accesses_network(mut self) -> Self {
        self.accesses_network = true;
        self
    }
}

/// Risk scorer for tool execution
pub struct ToolRiskScorer;

impl ToolRiskScorer {
    /// Calculate risk level for a tool
    pub fn calculate_risk(ctx: &ToolRiskContext) -> RiskLevel {
        let mut base_score = Self::base_risk_for_tool(&ctx.tool_name);

        // Apply modifiers
        if ctx.is_destructive {
            base_score += 30;
        }
        if ctx.is_write {
            base_score += 15;
        }
        if ctx.accesses_network {
            base_score += 10;
        }

        // Apply source multiplier
        base_score = (base_score as f32 * ctx.source.risk_multiplier()) as u32;

        // Apply trust reduction
        base_score = (base_score as f32 * ctx.workspace_trust.risk_reduction()) as u32;

        // Approval history reduces risk (diminishing returns)
        let approval_reduction = ctx.recent_approvals.min(3) as u32 * 5;
        base_score = base_score.saturating_sub(approval_reduction);

        // Convert to risk level
        match base_score {
            0..=25 => RiskLevel::Low,
            26..=50 => RiskLevel::Medium,
            51..=75 => RiskLevel::High,
            _ => RiskLevel::Critical,
        }
    }

    /// Determine if justification is required
    pub fn requires_justification(risk: RiskLevel, threshold: RiskLevel) -> bool {
        risk >= threshold
    }

    /// Base risk score for common tools
    fn base_risk_for_tool(tool_name: &str) -> u32 {
        match tool_name {
            // Read-only tools (base: 0)
            "read_file" | "list_files" | "grep_file" => 0,

            // Safe metadata tools (base: 5)
            "file_info" | "status" | "logs" => 5,

            // Write tools (base: 20)
            "write_file" | "edit_file" | "create_file" => 20,

            // Potentially risky write operations (base: 25)
            "apply_patch" | "delete_file" => 25,

            // Command execution (base: 30)
            "run_terminal_cmd" => 30,

            // PTY/interactive commands (base: 35)
            "create_pty_session" | "run_pty_cmd" | "send_pty_input" => 35,

            // Network operations (base: 40)
            "web_search" | "fetch_url" => 40,

            // MCP tools (default to medium risk)
            _ if tool_name.starts_with("mcp_") => 30,

            // Unknown tools default to medium-high risk
            _ => 35,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_level_ordering() {
        assert!(RiskLevel::Low < RiskLevel::Medium);
        assert!(RiskLevel::Medium < RiskLevel::High);
        assert!(RiskLevel::High < RiskLevel::Critical);
    }

    #[test]
    fn test_risk_calculation() {
        // Read-only operation in trusted workspace
        let ctx = ToolRiskContext::new(
            "read_file".to_string(),
            ToolSource::Internal,
            WorkspaceTrust::Trusted,
        );
        let risk = ToolRiskScorer::calculate_risk(&ctx);
        assert_eq!(risk, RiskLevel::Low);

        // Write operation in untrusted workspace
        let ctx = ToolRiskContext::new(
            "write_file".to_string(),
            ToolSource::External,
            WorkspaceTrust::Untrusted,
        )
        .as_write();
        let risk = ToolRiskScorer::calculate_risk(&ctx);
        assert!(risk >= RiskLevel::High);
    }

    #[test]
    fn test_approval_history_reduces_risk() {
        let mut ctx = ToolRiskContext::new(
            "run_terminal_cmd".to_string(),
            ToolSource::Internal,
            WorkspaceTrust::Untrusted,
        );

        let risk_before = ToolRiskScorer::calculate_risk(&ctx);

        ctx.recent_approvals = 3;
        let risk_after = ToolRiskScorer::calculate_risk(&ctx);

        assert!(risk_after <= risk_before);
    }

    #[test]
    fn test_source_multiplier() {
        let base = ToolRiskContext::new(
            "mcp_tool".to_string(),
            ToolSource::Internal,
            WorkspaceTrust::Trusted,
        );
        let base_risk = ToolRiskScorer::calculate_risk(&base);

        let mcp = ToolRiskContext::new(
            "mcp_tool".to_string(),
            ToolSource::Mcp,
            WorkspaceTrust::Trusted,
        );
        let mcp_risk = ToolRiskScorer::calculate_risk(&mcp);

        // MCP tool should have higher risk
        assert!(mcp_risk > base_risk || mcp_risk == RiskLevel::Critical);
    }

    #[test]
    fn test_requires_justification() {
        assert!(ToolRiskScorer::requires_justification(
            RiskLevel::High,
            RiskLevel::High
        ));
        assert!(!ToolRiskScorer::requires_justification(
            RiskLevel::Medium,
            RiskLevel::High
        ));
    }
}
