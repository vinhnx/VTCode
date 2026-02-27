use vtcode_config::NotificationDeliveryMode;

use crate::config::ToolOutputMode;
use crate::config::{AgentClientProtocolZedWorkspaceTrustMode, ToolPolicy, UiDisplayMode};

use super::{ConfigItem, ConfigItemKind, ConfigPalette};

impl ConfigPalette {
    #[allow(clippy::vec_init_then_push)]
    pub fn reload_items_from_config(&mut self) {
        let config = &self.config;
        let mut items = Vec::new();

        // -- Agent Behavior Section --

        // Reasoning Effort
        items.push(ConfigItem {
            key: "agent.reasoning_effort".to_string(),
            label: "Reasoning Effort".to_string(),
            kind: ConfigItemKind::Enum {
                value: config.agent.reasoning_effort.to_string(),
                options: vec![
                    "none".to_string(),
                    "minimal".to_string(),
                    "low".to_string(),
                    "medium".to_string(),
                    "high".to_string(),
                    "xhigh".to_string(),
                ],
            },
            description: Some("Model reasoning depth (e.g. for Gemini thinking)".to_string()),
        });

        // System Prompt Mode
        items.push(ConfigItem {
            key: "agent.system_prompt_mode".to_string(),
            label: "System Prompt Mode".to_string(),
            kind: ConfigItemKind::Enum {
                value: config.agent.system_prompt_mode.to_string(),
                options: vec![
                    "minimal".to_string(),
                    "lightweight".to_string(),
                    "default".to_string(),
                    "specialized".to_string(),
                ],
            },
            description: Some("Complexity of instructions sent to the model".to_string()),
        });

        // Tool Documentation Mode
        items.push(ConfigItem {
            key: "agent.tool_documentation_mode".to_string(),
            label: "Tool Doc Mode".to_string(),
            kind: ConfigItemKind::Enum {
                value: config.agent.tool_documentation_mode.to_string(),
                options: vec![
                    "minimal".to_string(),
                    "progressive".to_string(),
                    "full".to_string(),
                ],
            },
            description: Some("How much tool documentation to include in context".to_string()),
        });

        // Verbosity Level
        items.push(ConfigItem {
            key: "agent.verbosity".to_string(),
            label: "Verbosity Level".to_string(),
            kind: ConfigItemKind::Enum {
                value: config.agent.verbosity.to_string(),
                options: vec!["low".to_string(), "medium".to_string(), "high".to_string()],
            },
            description: Some("Control model verbosity and detail level".to_string()),
        });

        // -- Features Section --

        // Planning Mode
        items.push(ConfigItem {
            key: "agent.todo_planning_mode".to_string(),
            label: "Planning Mode".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.agent.todo_planning_mode,
            },
            description: Some("Enable planning mode and onboarding hints".to_string()),
        });

        // Checkpointing
        items.push(ConfigItem {
            key: "agent.checkpointing.enabled".to_string(),
            label: "Auto Checkpoints".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.agent.checkpointing.enabled,
            },
            description: Some("Take snapshots after each successful turn".to_string()),
        });

        // Small Model
        items.push(ConfigItem {
            key: "agent.small_model.enabled".to_string(),
            label: "Small Model Tier".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.agent.small_model.enabled,
            },
            description: Some("Use cheaper model for logs/reading (>80% savings)".to_string()),
        });

        // Vibe Coding
        items.push(ConfigItem {
            key: "agent.vibe_coding.enabled".to_string(),
            label: "Vibe Coding".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.agent.vibe_coding.enabled,
            },
            description: Some("Enable lazy/casual request support".to_string()),
        });

        // Prompt Caching
        items.push(ConfigItem {
            key: "prompt_cache.enabled".to_string(),
            label: "Prompt Caching".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.prompt_cache.enabled,
            },
            description: Some("Enable local prompt caching to reduce API costs".to_string()),
        });

        // MCP Support
        items.push(ConfigItem {
            key: "mcp.enabled".to_string(),
            label: "MCP Support".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.mcp.enabled,
            },
            description: Some("Enable Model Context Protocol support".to_string()),
        });

        // ACP Workspace Trust
        items.push(ConfigItem {
            key: "acp.zed.workspace_trust".to_string(),
            label: "ACP Workspace Trust".to_string(),
            kind: ConfigItemKind::Enum {
                value: match config.acp.zed.workspace_trust {
                    AgentClientProtocolZedWorkspaceTrustMode::FullAuto => "full_auto".to_string(),
                    AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy => {
                        "tools_policy".to_string()
                    }
                },
                options: vec!["tools_policy".to_string(), "full_auto".to_string()],
            },
            description: Some("Trust mode for ACP sessions (tools_policy/full_auto)".to_string()),
        });

        // -- Automation & Safety Section --

        // Full Auto
        items.push(ConfigItem {
            key: "automation.full_auto.enabled".to_string(),
            label: "Full Auto".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.automation.full_auto.enabled,
            },
            description: Some("Enable full-auto automation mode".to_string()),
        });

        // Default Tool Policy
        items.push(ConfigItem {
            key: "tools.default_policy".to_string(),
            label: "Tool Policy".to_string(),
            kind: ConfigItemKind::Enum {
                value: match config.tools.default_policy {
                    ToolPolicy::Allow => "allow".to_string(),
                    ToolPolicy::Prompt => "prompt".to_string(),
                    ToolPolicy::Deny => "deny".to_string(),
                },
                options: vec![
                    "allow".to_string(),
                    "prompt".to_string(),
                    "deny".to_string(),
                ],
            },
            description: Some("Default confirmation policy for tools".to_string()),
        });

        // Human In The Loop
        items.push(ConfigItem {
            key: "security.human_in_the_loop".to_string(),
            label: "Human In The Loop".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.security.human_in_the_loop,
            },
            description: Some("Require confirmations for critical actions".to_string()),
        });

        // -- Limits & Session Section --

        // Max Context Tokens
        items.push(ConfigItem {
            key: "context.max_context_tokens".to_string(),
            label: "Max Context Tokens".to_string(),
            kind: ConfigItemKind::Number {
                value: config.context.max_context_tokens as i64,
                min: 4096,
                max: 200000,
            },
            description: Some("Maximum tokens to preserve in conversation context".to_string()),
        });

        // Trim Context Percent
        items.push(ConfigItem {
            key: "context.trim_to_percent".to_string(),
            label: "Context Trim %".to_string(),
            kind: ConfigItemKind::Number {
                value: config.context.trim_to_percent as i64,
                min: 10,
                max: 95,
            },
            description: Some("Trim context to this percent when over budget".to_string()),
        });

        // Max Turns
        items.push(ConfigItem {
            key: "agent.max_conversation_turns".to_string(),
            label: "Max Turns".to_string(),
            kind: ConfigItemKind::Number {
                value: config.agent.max_conversation_turns as i64,
                min: 10,
                max: 500,
            },
            description: Some("Auto-terminate session after this many turns".to_string()),
        });

        // -- UI & Appearance Section --

        // Theme
        items.push(ConfigItem {
            key: "agent.theme".to_string(),
            label: "UI Theme".to_string(),
            kind: ConfigItemKind::Enum {
                value: config.agent.theme.clone(),
                options: crate::ui::theme::available_themes()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect(),
            },
            description: Some("UI color theme".to_string()),
        });

        // UI Display Mode
        items.push(ConfigItem {
            key: "ui.display_mode".to_string(),
            label: "UI Display Mode".to_string(),
            kind: ConfigItemKind::Enum {
                value: match config.ui.display_mode {
                    UiDisplayMode::Full => "full".to_string(),
                    UiDisplayMode::Minimal => "minimal".to_string(),
                    UiDisplayMode::Focused => "focused".to_string(),
                },
                options: vec![
                    "full".to_string(),
                    "minimal".to_string(),
                    "focused".to_string(),
                ],
            },
            description: Some("UI preset: full (all features), minimal, or focused".to_string()),
        });

        // Show Sidebar
        items.push(ConfigItem {
            key: "ui.show_sidebar".to_string(),
            label: "Show Sidebar".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.ui.show_sidebar,
            },
            description: Some("Show right pane with queue/context/tools".to_string()),
        });

        // Dim Completed Todos
        items.push(ConfigItem {
            key: "ui.dim_completed_todos".to_string(),
            label: "Dim Completed Todos".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.ui.dim_completed_todos,
            },
            description: Some("Dim completed todo items (- [x]) in output".to_string()),
        });

        // Message Block Spacing
        items.push(ConfigItem {
            key: "ui.message_block_spacing".to_string(),
            label: "Message Spacing".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.ui.message_block_spacing,
            },
            description: Some("Add blank lines between message blocks".to_string()),
        });

        // Show Turn Timer
        items.push(ConfigItem {
            key: "ui.show_turn_timer".to_string(),
            label: "Show Turn Timer".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.ui.show_turn_timer,
            },
            description: Some("Show elapsed-time divider after completed turns".to_string()),
        });

        // Tool Output Mode
        items.push(ConfigItem {
            key: "ui.tool_output_mode".to_string(),
            label: "Tool Output Mode".to_string(),
            kind: ConfigItemKind::Enum {
                value: match config.ui.tool_output_mode {
                    ToolOutputMode::Compact => "compact".to_string(),
                    ToolOutputMode::Full => "full".to_string(),
                },
                options: vec!["compact".to_string(), "full".to_string()],
            },
            description: Some("Control verbosity of tool output".to_string()),
        });

        // Allow Tool ANSI
        items.push(ConfigItem {
            key: "ui.allow_tool_ansi".to_string(),
            label: "Allow Tool ANSI".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.ui.allow_tool_ansi,
            },
            description: Some("Preserve ANSI color codes from tool output".to_string()),
        });

        // Notifications Enabled
        items.push(ConfigItem {
            key: "ui.notifications.enabled".to_string(),
            label: "Notifications Enabled".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.ui.notifications.enabled,
            },
            description: Some("Enable runtime notifications for critical agent events".to_string()),
        });

        // Notification Delivery Mode
        items.push(ConfigItem {
            key: "ui.notifications.delivery_mode".to_string(),
            label: "Notification Delivery".to_string(),
            kind: ConfigItemKind::Enum {
                value: match config.ui.notifications.delivery_mode {
                    NotificationDeliveryMode::Terminal => "terminal".to_string(),
                    NotificationDeliveryMode::Hybrid => "hybrid".to_string(),
                    NotificationDeliveryMode::Desktop => "desktop".to_string(),
                },
                options: vec![
                    "terminal".to_string(),
                    "hybrid".to_string(),
                    "desktop".to_string(),
                ],
            },
            description: Some(
                "Delivery mode: terminal bell/OSC, hybrid, or desktop-first".to_string(),
            ),
        });

        // Notification focus suppression
        items.push(ConfigItem {
            key: "ui.notifications.suppress_when_focused".to_string(),
            label: "Notify In Background Only".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.ui.notifications.suppress_when_focused,
            },
            description: Some("Suppress notifications while terminal is focused".to_string()),
        });

        // Tool failure notifications
        items.push(ConfigItem {
            key: "ui.notifications.tool_failure".to_string(),
            label: "Notify Tool Failures".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.ui.notifications.tool_failure,
            },
            description: Some("Alert when tool execution fails".to_string()),
        });

        // Error notifications
        items.push(ConfigItem {
            key: "ui.notifications.error".to_string(),
            label: "Notify Errors".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.ui.notifications.error,
            },
            description: Some("Alert on runtime/system errors".to_string()),
        });

        // Completion notifications
        items.push(ConfigItem {
            key: "ui.notifications.completion".to_string(),
            label: "Notify Completion".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.ui.notifications.completion,
            },
            description: Some("Alert when turn/session completes".to_string()),
        });

        // HITL notifications
        items.push(ConfigItem {
            key: "ui.notifications.hitl".to_string(),
            label: "Notify HITL Prompts".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.ui.notifications.hitl,
            },
            description: Some("Alert when approval/user input is required".to_string()),
        });

        // Tool success notifications
        items.push(ConfigItem {
            key: "ui.notifications.tool_success".to_string(),
            label: "Notify Tool Success".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.ui.notifications.tool_success,
            },
            description: Some("Alert for successful tool calls (can be noisy)".to_string()),
        });

        // Syntax Highlighting
        items.push(ConfigItem {
            key: "syntax_highlighting.enabled".to_string(),
            label: "Syntax Highlighting".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.syntax_highlighting.enabled,
            },
            description: Some("Enable syntax highlighting in UI output".to_string()),
        });

        // -- Keyboard Protocol Section --

        // Keyboard Protocol Enabled
        items.push(ConfigItem {
            key: "ui.keyboard_protocol.enabled".to_string(),
            label: "Keyboard Protocol".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.ui.keyboard_protocol.enabled,
            },
            description: Some("Enable kitty keyboard protocol enhancements".to_string()),
        });

        // Keyboard Protocol Mode
        items.push(ConfigItem {
            key: "ui.keyboard_protocol.mode".to_string(),
            label: "Keyboard Mode".to_string(),
            kind: ConfigItemKind::Enum {
                value: config.ui.keyboard_protocol.mode.clone(),
                options: vec![
                    "default".to_string(),
                    "full".to_string(),
                    "minimal".to_string(),
                    "custom".to_string(),
                ],
            },
            description: Some(
                "Keyboard enhancement preset (default/full/minimal/custom)".to_string(),
            ),
        });

        // -- Internal Section --

        // Inline Viewport Rows
        items.push(ConfigItem {
            key: "ui.inline_viewport_rows".to_string(),
            label: "Viewport Rows".to_string(),
            kind: ConfigItemKind::Number {
                value: config.ui.inline_viewport_rows as i64,
                min: 5,
                max: 50,
            },
            description: Some("Height of the main TUI viewport".to_string()),
        });

        // PTY Enabled
        items.push(ConfigItem {
            key: "pty.enabled".to_string(),
            label: "PTY Enabled".to_string(),
            kind: ConfigItemKind::Bool {
                value: config.pty.enabled,
            },
            description: Some("Enable PTY-backed command execution".to_string()),
        });

        // PTY Rows
        items.push(ConfigItem {
            key: "pty.default_rows".to_string(),
            label: "PTY Rows".to_string(),
            kind: ConfigItemKind::Number {
                value: config.pty.default_rows as i64,
                min: 10,
                max: 100,
            },
            description: Some("Default rows for PTY sessions".to_string()),
        });

        // PTY Columns
        items.push(ConfigItem {
            key: "pty.default_cols".to_string(),
            label: "PTY Cols".to_string(),
            kind: ConfigItemKind::Number {
                value: config.pty.default_cols as i64,
                min: 40,
                max: 200,
            },
            description: Some("Default columns for PTY sessions".to_string()),
        });

        // PTY Timeout
        items.push(ConfigItem {
            key: "pty.command_timeout_seconds".to_string(),
            label: "PTY Timeout (s)".to_string(),
            kind: ConfigItemKind::Number {
                value: config.pty.command_timeout_seconds as i64,
                min: 10,
                max: 3600,
            },
            description: Some("Command timeout for PTY sessions".to_string()),
        });

        // Read-only model info
        items.push(ConfigItem {
            key: "agent.default_model".to_string(),
            label: "Active Model".to_string(),
            kind: ConfigItemKind::Display {
                value: config.agent.default_model.clone(),
            },
            description: Some(
                "Main AI model (read-only), to change model, please use /model command."
                    .to_string(),
            ),
        });

        self.items = items;

        // Ensure selection is within bounds
        if let Some(selected) = self.list_state.selected() {
            if selected >= self.items.len() && !self.items.is_empty() {
                self.list_state.select(Some(self.items.len() - 1));
            } else if self.items.is_empty() {
                self.list_state.select(None);
            }
        } else if !self.items.is_empty() {
            self.list_state.select(Some(0));
        }
    }
}
