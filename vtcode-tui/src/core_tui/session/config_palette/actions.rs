use vtcode_config::NotificationDeliveryMode;

use crate::config::ToolOutputMode;
use crate::config::{
    AgentClientProtocolZedWorkspaceTrustMode, ReasoningEffortLevel, SystemPromptMode,
    ToolDocumentationMode, ToolPolicy, UiDisplayMode, VerbosityLevel,
};

use super::ConfigPalette;

impl ConfigPalette {
    pub fn adjust_numeric_val(&mut self, delta: i64) {
        if let Some(index) = self.selected()
            && let Some(item) = self.items.get(index)
        {
            let key = item.key.clone();
            let mut changed = false;

            match key.as_str() {
                "ui.inline_viewport_rows" => {
                    let val = (self.config.ui.inline_viewport_rows as i64 + delta).clamp(5, 50);
                    self.config.ui.inline_viewport_rows = val as u16;
                    changed = true;
                }
                "pty.default_rows" => {
                    let val = (self.config.pty.default_rows as i64 + delta).clamp(10, 100);
                    self.config.pty.default_rows = val as u16;
                    changed = true;
                }
                "pty.default_cols" => {
                    let val = (self.config.pty.default_cols as i64 + delta).clamp(40, 200);
                    self.config.pty.default_cols = val as u16;
                    changed = true;
                }
                "pty.command_timeout_seconds" => {
                    let val =
                        (self.config.pty.command_timeout_seconds as i64 + delta).clamp(10, 3600);
                    self.config.pty.command_timeout_seconds = val as u64;
                    changed = true;
                }
                "context.max_context_tokens" => {
                    // Use larger step for tokens: 1024 if small delta, otherwise as is
                    let step = if delta.abs() == 1 {
                        1024 * delta
                    } else {
                        delta
                    };
                    let val =
                        (self.config.context.max_context_tokens as i64 + step).clamp(4096, 200000);
                    self.config.context.max_context_tokens = val as usize;
                    changed = true;
                }
                "context.trim_to_percent" => {
                    let val = (self.config.context.trim_to_percent as i64 + delta).clamp(10, 95);
                    self.config.context.trim_to_percent = val as u8;
                    changed = true;
                }
                "agent.max_conversation_turns" => {
                    let val =
                        (self.config.agent.max_conversation_turns as i64 + delta).clamp(10, 500);
                    self.config.agent.max_conversation_turns = val as usize;
                    changed = true;
                }
                _ => {}
            }

            if changed {
                self.modified = true;
                self.reload_items_from_config();
            }
        }
    }

    pub fn toggle_selected(&mut self) {
        if let Some(index) = self.selected()
            && let Some(item) = self.items.get(index)
        {
            let key = item.key.clone();
            let mut changed = false;

            match key.as_str() {
                "ui.tool_output_mode" => {
                    self.config.ui.tool_output_mode = match self.config.ui.tool_output_mode {
                        ToolOutputMode::Compact => ToolOutputMode::Full,
                        ToolOutputMode::Full => ToolOutputMode::Compact,
                    };
                    changed = true;
                }
                "ui.allow_tool_ansi" => {
                    self.config.ui.allow_tool_ansi = !self.config.ui.allow_tool_ansi;
                    changed = true;
                }
                "ui.notifications.enabled" => {
                    self.config.ui.notifications.enabled = !self.config.ui.notifications.enabled;
                    changed = true;
                }
                "ui.notifications.delivery_mode" => {
                    self.config.ui.notifications.delivery_mode =
                        match self.config.ui.notifications.delivery_mode {
                            NotificationDeliveryMode::Terminal => NotificationDeliveryMode::Hybrid,
                            NotificationDeliveryMode::Hybrid => NotificationDeliveryMode::Desktop,
                            NotificationDeliveryMode::Desktop => NotificationDeliveryMode::Terminal,
                        };
                    changed = true;
                }
                "ui.notifications.suppress_when_focused" => {
                    self.config.ui.notifications.suppress_when_focused =
                        !self.config.ui.notifications.suppress_when_focused;
                    changed = true;
                }
                "ui.notifications.tool_failure" => {
                    self.config.ui.notifications.tool_failure =
                        !self.config.ui.notifications.tool_failure;
                    changed = true;
                }
                "ui.notifications.error" => {
                    self.config.ui.notifications.error = !self.config.ui.notifications.error;
                    changed = true;
                }
                "ui.notifications.completion" => {
                    self.config.ui.notifications.completion =
                        !self.config.ui.notifications.completion;
                    changed = true;
                }
                "ui.notifications.hitl" => {
                    self.config.ui.notifications.hitl = !self.config.ui.notifications.hitl;
                    changed = true;
                }
                "ui.notifications.tool_success" => {
                    self.config.ui.notifications.tool_success =
                        !self.config.ui.notifications.tool_success;
                    changed = true;
                }
                "agent.theme" => {
                    let themes = crate::ui::theme::available_themes();
                    if !themes.is_empty() {
                        let current = &self.config.agent.theme;
                        let next_index = themes
                            .iter()
                            .position(|&t| t == current)
                            .map(|index| (index + 1) % themes.len())
                            .unwrap_or(0);
                        self.config.agent.theme = themes[next_index].to_string();
                        changed = true;
                    }
                }
                "pty.enabled" => {
                    self.config.pty.enabled = !self.config.pty.enabled;
                    changed = true;
                }
                "security.human_in_the_loop" => {
                    self.config.security.human_in_the_loop =
                        !self.config.security.human_in_the_loop;
                    changed = true;
                }
                "tools.default_policy" => {
                    self.config.tools.default_policy = match self.config.tools.default_policy {
                        ToolPolicy::Allow => ToolPolicy::Prompt,
                        ToolPolicy::Prompt => ToolPolicy::Deny,
                        ToolPolicy::Deny => ToolPolicy::Allow,
                    };
                    changed = true;
                }
                "agent.reasoning_effort" => {
                    self.config.agent.reasoning_effort = match self.config.agent.reasoning_effort {
                        ReasoningEffortLevel::None => ReasoningEffortLevel::Minimal,
                        ReasoningEffortLevel::Minimal => ReasoningEffortLevel::Low,
                        ReasoningEffortLevel::Low => ReasoningEffortLevel::Medium,
                        ReasoningEffortLevel::Medium => ReasoningEffortLevel::High,
                        ReasoningEffortLevel::High => ReasoningEffortLevel::XHigh,
                        ReasoningEffortLevel::XHigh => ReasoningEffortLevel::None,
                    };
                    changed = true;
                }
                "agent.system_prompt_mode" => {
                    self.config.agent.system_prompt_mode =
                        match self.config.agent.system_prompt_mode {
                            SystemPromptMode::Minimal => SystemPromptMode::Lightweight,
                            SystemPromptMode::Lightweight => SystemPromptMode::Default,
                            SystemPromptMode::Default => SystemPromptMode::Specialized,
                            SystemPromptMode::Specialized => SystemPromptMode::Minimal,
                        };
                    changed = true;
                }
                "agent.tool_documentation_mode" => {
                    self.config.agent.tool_documentation_mode =
                        match self.config.agent.tool_documentation_mode {
                            ToolDocumentationMode::Minimal => ToolDocumentationMode::Progressive,
                            ToolDocumentationMode::Progressive => ToolDocumentationMode::Full,
                            ToolDocumentationMode::Full => ToolDocumentationMode::Minimal,
                        };
                    changed = true;
                }
                "agent.verbosity" => {
                    self.config.agent.verbosity = match self.config.agent.verbosity {
                        VerbosityLevel::Low => VerbosityLevel::Medium,
                        VerbosityLevel::Medium => VerbosityLevel::High,
                        VerbosityLevel::High => VerbosityLevel::Low,
                    };
                    changed = true;
                }
                "agent.todo_planning_mode" => {
                    self.config.agent.todo_planning_mode = !self.config.agent.todo_planning_mode;
                    changed = true;
                }
                "agent.checkpointing.enabled" => {
                    self.config.agent.checkpointing.enabled =
                        !self.config.agent.checkpointing.enabled;
                    changed = true;
                }
                "agent.small_model.enabled" => {
                    self.config.agent.small_model.enabled = !self.config.agent.small_model.enabled;
                    changed = true;
                }
                "agent.vibe_coding.enabled" => {
                    self.config.agent.vibe_coding.enabled = !self.config.agent.vibe_coding.enabled;
                    changed = true;
                }
                "syntax_highlighting.enabled" => {
                    self.config.syntax_highlighting.enabled =
                        !self.config.syntax_highlighting.enabled;
                    changed = true;
                }
                "automation.full_auto.enabled" => {
                    self.config.automation.full_auto.enabled =
                        !self.config.automation.full_auto.enabled;
                    changed = true;
                }
                "prompt_cache.enabled" => {
                    self.config.prompt_cache.enabled = !self.config.prompt_cache.enabled;
                    changed = true;
                }
                "mcp.enabled" => {
                    self.config.mcp.enabled = !self.config.mcp.enabled;
                    changed = true;
                }
                "acp.zed.workspace_trust" => {
                    self.config.acp.zed.workspace_trust = match self.config.acp.zed.workspace_trust
                    {
                        AgentClientProtocolZedWorkspaceTrustMode::FullAuto => {
                            AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy
                        }
                        AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy => {
                            AgentClientProtocolZedWorkspaceTrustMode::FullAuto
                        }
                    };
                    changed = true;
                }
                "ui.keyboard_protocol.enabled" => {
                    self.config.ui.keyboard_protocol.enabled =
                        !self.config.ui.keyboard_protocol.enabled;
                    changed = true;
                }
                "ui.keyboard_protocol.mode" => {
                    self.config.ui.keyboard_protocol.mode =
                        match self.config.ui.keyboard_protocol.mode.as_str() {
                            "default" => "full".to_string(),
                            "full" => "minimal".to_string(),
                            "minimal" => "custom".to_string(),
                            "custom" => "default".to_string(),
                            _ => "default".to_string(),
                        };
                    changed = true;
                }
                "ui.display_mode" => {
                    self.config.ui.display_mode = match self.config.ui.display_mode {
                        UiDisplayMode::Full => UiDisplayMode::Minimal,
                        UiDisplayMode::Minimal => UiDisplayMode::Focused,
                        UiDisplayMode::Focused => UiDisplayMode::Full,
                    };
                    changed = true;
                }
                "ui.show_sidebar" => {
                    self.config.ui.show_sidebar = !self.config.ui.show_sidebar;
                    changed = true;
                }
                "ui.dim_completed_todos" => {
                    self.config.ui.dim_completed_todos = !self.config.ui.dim_completed_todos;
                    changed = true;
                }
                "ui.message_block_spacing" => {
                    self.config.ui.message_block_spacing = !self.config.ui.message_block_spacing;
                    changed = true;
                }
                "ui.show_turn_timer" => {
                    self.config.ui.show_turn_timer = !self.config.ui.show_turn_timer;
                    changed = true;
                }
                "ui.inline_viewport_rows"
                | "pty.default_rows"
                | "pty.default_cols"
                | "pty.command_timeout_seconds"
                | "context.max_context_tokens"
                | "context.trim_to_percent"
                | "agent.max_conversation_turns" => {
                    self.adjust_numeric_val(1);
                }
                _ => {}
            }

            if changed {
                self.modified = true;
                self.reload_items_from_config();
            }
        }
    }
}
