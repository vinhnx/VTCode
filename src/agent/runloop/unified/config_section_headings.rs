use std::borrow::Cow;

use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug, Clone)]
pub(super) struct SectionHeading {
    pub(super) title: Cow<'static, str>,
    pub(super) summary: Cow<'static, str>,
}

static ARRAY_INDEX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[(\d+)\]").expect("valid regex"));

static SECTION_HEADINGS: &[(&str, &str, &str)] = &[
    (
        "acp",
        "ACP Bridge",
        "IDE and Agent Client Protocol integrations.",
    ),
    (
        "acp.zed",
        "Zed Integration",
        "How VT Code connects commands and trust settings to Zed.",
    ),
    (
        "acp.zed.auth",
        "Zed Authentication",
        "Default authentication flow for ACP sessions in Zed.",
    ),
    (
        "acp.zed.tools",
        "Zed Tool Bridge",
        "Which file tools are exposed to the Zed client.",
    ),
    (
        "agent",
        "Agent Defaults",
        "Primary model, prompt, and chat behavior.",
    ),
    (
        "agent.checkpointing",
        "Checkpoints",
        "Automatic turn snapshots and retention limits.",
    ),
    (
        "agent.circuit_breaker",
        "Circuit Breaker",
        "Failure pause thresholds and recovery guardrails.",
    ),
    (
        "agent.harness",
        "Harness Limits",
        "Per-turn tool call and timing safety limits.",
    ),
    (
        "agent.onboarding",
        "Onboarding",
        "Welcome message, tips, and first-run guidance.",
    ),
    (
        "agent.open_responses",
        "Open Responses",
        "Compatibility events and response item mapping.",
    ),
    (
        "agent.prompt_suggestions",
        "Prompt Suggestions",
        "Inline composer suggestions and lightweight model routing.",
    ),
    (
        "agent.persistent_memory",
        "Persistent Memory",
        "Per-repository memory summary, rollout staging, and learned durable notes.",
    ),
    (
        "agent.small_model",
        "Lightweight Model Helpers",
        "Low-cost lightweight-model routing for memory and other side tasks.",
    ),
    (
        "agent.vibe_coding",
        "Vibe Coding",
        "Loose-input parsing and extra contextual assistance.",
    ),
    (
        "auth",
        "Authentication",
        "Provider login and token handling.",
    ),
    (
        "auth.openrouter",
        "OpenRouter OAuth",
        "Browser login flow and token refresh behavior.",
    ),
    (
        "automation",
        "Automation",
        "Autonomous run controls and safety defaults.",
    ),
    (
        "automation.full_auto",
        "Full Auto",
        "Guardrails for unattended execution.",
    ),
    (
        "automation.scheduled_tasks",
        "Scheduled Tasks",
        "Session-scoped loops and durable local automation jobs.",
    ),
    (
        "chat",
        "Chat Surface",
        "Interactive chat features and assistant affordances.",
    ),
    (
        "chat.askQuestions",
        "Ask Questions Tool",
        "Inline clarification prompts during chat sessions.",
    ),
    (
        "commands",
        "Command Policies",
        "Allow and deny rules for shell execution.",
    ),
    (
        "custom_providers",
        "Custom Providers",
        "Named OpenAI-compatible endpoints configured in vtcode.toml.",
    ),
    (
        "custom_providers[]",
        "Custom Provider",
        "Display name, base URL, and API key settings for one endpoint.",
    ),
    (
        "context",
        "Context Budget",
        "How much history and workspace context is retained.",
    ),
    (
        "context.dynamic",
        "Dynamic Context",
        "Spooled outputs and generated workspace context artifacts.",
    ),
    (
        "context.ledger",
        "Decision Ledger",
        "Important decisions kept across the session.",
    ),
    (
        "debug",
        "Debug Logging",
        "Tracing and local diagnostics output.",
    ),
    (
        "dotfile_protection",
        "Dotfile Protection",
        "Extra safeguards for shell and editor dotfiles.",
    ),
    (
        "hooks",
        "Lifecycle Hooks",
        "Automations triggered around session events.",
    ),
    (
        "hooks.lifecycle",
        "Lifecycle Events",
        "Hook groups for session and tool milestones.",
    ),
    (
        "ide_context",
        "IDE Context",
        "Cross-IDE active editor context injected into prompts and inline UI.",
    ),
    (
        "ide_context.providers",
        "IDE Providers",
        "Per-editor-family enablement for editor context ingestion.",
    ),
    (
        "ide_context.providers.vscode_compatible",
        "VS Code Family",
        "Enable context snapshots from VS Code, Cursor, Windsurf, and compatible editors.",
    ),
    (
        "ide_context.providers.zed",
        "Zed Family",
        "Enable context snapshots from Zed integrations.",
    ),
    (
        "ide_context.providers.generic",
        "Generic Bridge",
        "Enable file-bridge snapshots from JetBrains and other external adapters.",
    ),
    (
        "mcp",
        "MCP Servers",
        "Model Context Protocol providers and defaults.",
    ),
    (
        "mcp.allowlist",
        "MCP Allowlist",
        "Default restrictions for MCP resources, prompts, and config.",
    ),
    (
        "mcp.allowlist.default",
        "Default MCP Rules",
        "Fallback allowlist applied to every MCP provider.",
    ),
    (
        "mcp.allowlist.providers",
        "Provider Overrides",
        "Per-provider MCP allowlist exceptions.",
    ),
    (
        "mcp.requirements",
        "MCP Requirements",
        "Startup checks and required capabilities for MCP providers.",
    ),
    (
        "mcp.security",
        "MCP Security",
        "Validation and runtime protection for MCP.",
    ),
    (
        "mcp.security.rate_limit",
        "MCP Rate Limits",
        "Concurrency and throughput guardrails for MCP traffic.",
    ),
    (
        "mcp.security.validation",
        "MCP Validation",
        "Schema and payload checks for MCP requests.",
    ),
    (
        "mcp.server",
        "MCP Server Transport",
        "How VT Code hosts its own MCP server.",
    ),
    (
        "mcp.ui",
        "MCP UI",
        "How MCP tools and events appear in the interface.",
    ),
    (
        "mcp.ui.renderers",
        "MCP Renderers",
        "Presentation rules for specific MCP tool outputs.",
    ),
    (
        "model",
        "Model Defaults",
        "Shared model selection and context settings.",
    ),
    (
        "model_config",
        "Model Config",
        "Focused main-model and lightweight-model configuration.",
    ),
    (
        "model_config.main",
        "Main Model",
        "Provider and default model for the active conversation model.",
    ),
    (
        "model_config.lightweight",
        "Lightweight Model",
        "Shared lower-cost route for memory, prompt suggestions, and smaller delegated tasks.",
    ),
    (
        "optimization",
        "Performance",
        "Caching and runtime performance tuning.",
    ),
    (
        "optimization.agent_execution",
        "Agent Execution",
        "Concurrency and execution scheduling for agent turns.",
    ),
    (
        "optimization.async_pipeline",
        "Async Pipeline",
        "Background streaming and async pipeline tuning.",
    ),
    (
        "optimization.command_cache",
        "Command Cache",
        "Cache repeated shell command results.",
    ),
    (
        "optimization.file_read_cache",
        "File Read Cache",
        "Reuse hot file reads to reduce I/O churn.",
    ),
    (
        "optimization.llm_client",
        "LLM Client",
        "HTTP client behavior for model requests.",
    ),
    (
        "optimization.memory_pool",
        "Memory Pool",
        "Allocator and buffer reuse settings.",
    ),
    (
        "optimization.profiling",
        "Profiling",
        "Performance instrumentation and trace sampling.",
    ),
    (
        "optimization.tool_registry",
        "Tool Registry",
        "Startup and lookup behavior for tool metadata.",
    ),
    (
        "output_style",
        "Output Style",
        "Formatting and readability preferences.",
    ),
    (
        "permissions",
        "Permissions",
        "Default approval and trust behavior.",
    ),
    (
        "prompt_cache",
        "Prompt Cache",
        "Reuse prompts to cut duplicate provider work.",
    ),
    (
        "prompt_cache.providers",
        "Prompt Cache Providers",
        "Provider-specific prompt cache behavior.",
    ),
    (
        "provider",
        "Provider Endpoints",
        "Network and provider-specific API settings.",
    ),
    (
        "provider.anthropic",
        "Anthropic",
        "Anthropic-specific request settings.",
    ),
    (
        "provider.anthropic.tool_search",
        "Anthropic Tool Search",
        "Hosted search tool configuration for Anthropic models.",
    ),
    (
        "provider.openai",
        "OpenAI",
        "OpenAI-specific request settings.",
    ),
    (
        "provider.openai.tool_search",
        "OpenAI Tool Search",
        "Hosted search tool configuration for OpenAI Responses models.",
    ),
    (
        "pty",
        "Terminal Sessions",
        "PTY limits, buffering, and shell defaults.",
    ),
    (
        "sandbox",
        "Sandboxing",
        "Process isolation and filesystem boundaries.",
    ),
    (
        "sandbox.external",
        "External Sandbox",
        "Container or VM-backed isolation settings.",
    ),
    (
        "sandbox.external.docker",
        "Docker Sandbox",
        "Docker-based sandbox execution settings.",
    ),
    (
        "sandbox.external.microvm",
        "MicroVM Sandbox",
        "MicroVM-backed isolation settings.",
    ),
    (
        "sandbox.network",
        "Sandbox Network",
        "Outbound network restrictions inside the sandbox.",
    ),
    (
        "sandbox.resource_limits",
        "Resource Limits",
        "CPU, memory, and disk ceilings for sandbox runs.",
    ),
    (
        "sandbox.seccomp",
        "Seccomp Filters",
        "Linux syscall restrictions for sandboxed processes.",
    ),
    (
        "sandbox.sensitive_paths",
        "Sensitive Paths",
        "Protected host paths denied to sandboxed runs.",
    ),
    (
        "security",
        "Security Gate",
        "Approvals and blocking rules for risky actions.",
    ),
    (
        "security.gatekeeper",
        "Gatekeeper",
        "Extra approval checks and enforcement rules.",
    ),
    (
        "skills",
        "Skills",
        "Skill discovery, rendering, and loading behavior.",
    ),
    (
        "syntax_highlighting",
        "Syntax Highlighting",
        "Language-aware rendering in the transcript.",
    ),
    (
        "telemetry",
        "Telemetry",
        "Local analytics, metrics, and reporting.",
    ),
    ("timeouts", "Timeouts", "Global operation time limits."),
    (
        "tools",
        "Tool Defaults",
        "Loop limits and default tool behavior.",
    ),
    (
        "tools.editor",
        "External Editor",
        "Editor launch command and editor fallback.",
    ),
    (
        "tools.loop_thresholds",
        "Loop Thresholds",
        "Warn or stop when repeated tool loops are detected.",
    ),
    (
        "tools.plugins",
        "Tool Plugins",
        "Plugin loading and trust settings.",
    ),
    (
        "tools.policies",
        "Tool Policies",
        "Per-tool allow, prompt, or deny rules.",
    ),
    (
        "tools.web_fetch",
        "Web Fetch",
        "Remote fetch limits and safety checks.",
    ),
    (
        "ui",
        "Interface",
        "Appearance, layout, and transcript behavior.",
    ),
    (
        "ui.keyboard_protocol",
        "Keyboard Protocol",
        "Enhanced terminal keyboard reporting.",
    ),
    (
        "ui.notifications",
        "Notifications",
        "Desktop and in-app notification delivery.",
    ),
    (
        "ui.status_line",
        "Status Line",
        "Bottom status bar content and refresh behavior.",
    ),
];

pub(super) fn normalize_config_path(path: &str) -> String {
    ARRAY_INDEX_RE.replace_all(path, "[]").to_string()
}

pub(super) fn heading_for_path(path: &str) -> SectionHeading {
    let normalized_path = normalize_config_path(path);
    if let Some((_, title, summary)) = SECTION_HEADINGS
        .iter()
        .find(|(key, _, _)| *key == normalized_path)
    {
        return SectionHeading {
            title: Cow::Borrowed(title),
            summary: Cow::Borrowed(summary),
        };
    }

    if let Some(provider) = normalized_path.strip_prefix("prompt_cache.providers.") {
        return SectionHeading {
            title: Cow::Owned(format!("{} Prompt Cache", humanize_identifier(provider))),
            summary: Cow::Borrowed("Provider-specific prompt cache overrides."),
        };
    }

    if let Some(provider) = normalized_path.strip_prefix("mcp.allowlist.providers.") {
        return SectionHeading {
            title: Cow::Owned(format!("{} Allowlist", humanize_identifier(provider))),
            summary: Cow::Borrowed("MCP allowlist overrides for this provider."),
        };
    }

    let title = normalized_path
        .rsplit('.')
        .next()
        .map(humanize_identifier)
        .unwrap_or_else(|| "Settings".to_string());
    SectionHeading {
        title: Cow::Owned(title),
        summary: Cow::Borrowed(""),
    }
}

pub(super) fn humanize_identifier(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }

    let mut normalized = String::with_capacity(value.len() + 8);
    let mut previous_is_lower_or_digit = false;
    for ch in value.chars() {
        if matches!(ch, '_' | '-' | '.') {
            if !normalized.ends_with(' ') {
                normalized.push(' ');
            }
            previous_is_lower_or_digit = false;
            continue;
        }

        if ch.is_ascii_uppercase() && previous_is_lower_or_digit && !normalized.ends_with(' ') {
            normalized.push(' ');
        }

        previous_is_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        normalized.push(ch);
    }

    normalized
        .split_whitespace()
        .map(format_human_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_human_token(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    match lower.as_str() {
        "acp" => "ACP".to_string(),
        "api" => "API".to_string(),
        "llm" => "LLM".to_string(),
        "mcp" => "MCP".to_string(),
        "oauth" => "OAuth".to_string(),
        "pty" => "PTY".to_string(),
        "ui" => "UI".to_string(),
        "vtcode" => "VT Code".to_string(),
        "openai" => "OpenAI".to_string(),
        "openrouter" => "OpenRouter".to_string(),
        "deepseek" => "DeepSeek".to_string(),
        "moonshot" => "Moonshot".to_string(),
        "zai" => "Z.AI".to_string(),
        _ => {
            let mut chars = lower.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_config_path_replaces_array_indexes() {
        assert_eq!(
            normalize_config_path("hooks.lifecycle.pre_tool_use[12].hooks[1]"),
            "hooks.lifecycle.pre_tool_use[].hooks[]"
        );
    }

    #[test]
    fn humanize_identifier_preserves_acronyms_and_camel_case() {
        assert_eq!(humanize_identifier("askQuestions"), "Ask Questions");
        assert_eq!(humanize_identifier("mcp_ui"), "MCP UI");
        assert_eq!(humanize_identifier("openai"), "OpenAI");
    }

    #[test]
    fn heading_for_path_builds_dynamic_provider_titles() {
        let heading = heading_for_path("prompt_cache.providers.openrouter");
        assert_eq!(heading.title, "OpenRouter Prompt Cache");
        assert_eq!(heading.summary, "Provider-specific prompt cache overrides.");
    }
}
