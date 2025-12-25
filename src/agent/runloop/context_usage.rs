//! Context usage visualization for /context command
//!
//! Provides detailed breakdown of context token usage including:
//! - Visual token usage bar with symbols
//! - Grouped skills and agents by source
//! - Slash commands listing
//! - MCP tools listing
//! - Memory files

use anyhow::Result;
use vtcode_core::ui::slash::SLASH_COMMANDS;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

/// Symbols for the visual context bar
pub const SYMBOL_FILLED: char = '⛁'; // Filled/used space
pub const SYMBOL_FREE: char = '⛶'; // Free space
pub const SYMBOL_BUFFER: char = '⛝'; // Autocompact buffer

/// Context item with name and token count
#[derive(Debug, Clone)]
pub struct ContextItem {
    pub name: String,
    pub tokens: usize,
}

impl ContextItem {
    pub fn new(name: impl Into<String>, tokens: usize) -> Self {
        Self {
            name: name.into(),
            tokens,
        }
    }
}

/// Context usage breakdown by category
#[derive(Debug, Clone, Default)]
pub struct ContextUsageInfo {
    pub model_name: String,
    pub current_tokens: usize,
    pub max_tokens: usize,

    // Token breakdown
    pub system_prompt_tokens: usize,
    pub system_tools_tokens: usize,
    pub mcp_tools_tokens: usize,
    pub custom_agents_tokens: usize,
    pub memory_files_tokens: usize,
    pub messages_tokens: usize,
    pub autocompact_buffer_percent: f64,

    // Detailed items (sorted by token count descending)
    pub mcp_tools: Vec<ContextItem>,
    pub custom_agents: Vec<ContextItem>,
    pub memory_files: Vec<ContextItem>,
    pub user_skills: Vec<ContextItem>,
    pub project_skills: Vec<ContextItem>,
}

impl ContextUsageInfo {
    /// Create a new context usage info with basic configuration
    pub fn new(model_name: impl Into<String>, max_tokens: usize) -> Self {
        Self {
            model_name: model_name.into(),
            max_tokens,
            autocompact_buffer_percent: 22.5, // Default buffer percentage
            ..Default::default()
        }
    }

    /// Calculate the free space in tokens
    pub fn free_tokens(&self) -> usize {
        let used = self.system_prompt_tokens
            + self.system_tools_tokens
            + self.mcp_tools_tokens
            + self.custom_agents_tokens
            + self.memory_files_tokens
            + self.messages_tokens;
        self.max_tokens.saturating_sub(used)
    }

    /// Calculate usage percentage
    pub fn usage_percent(&self) -> f64 {
        if self.max_tokens == 0 {
            return 0.0;
        }
        (self.current_tokens as f64 / self.max_tokens as f64) * 100.0
    }


    /// Add MCP tool
    pub fn add_mcp_tool(&mut self, name: impl Into<String>, tokens: usize) {
        self.mcp_tools.push(ContextItem::new(name, tokens));
        self.mcp_tools.sort_by(|a, b| b.tokens.cmp(&a.tokens));
        self.mcp_tools_tokens = self.mcp_tools.iter().map(|t| t.tokens).sum();
    }

}


/// Format token count for display (e.g., "3.2k" or "150")
fn format_tokens(tokens: usize) -> String {
    if tokens >= 1000 {
        format!("{:.1}k", tokens as f64 / 1000.0)
    } else {
        tokens.to_string()
    }
}

/// Render the context usage visualization
pub fn render_context_usage(
    renderer: &mut AnsiRenderer,
    info: &ContextUsageInfo,
) -> Result<()> {
    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Info, "Context Usage")?;

    // Render visual token bar
    render_token_bar(renderer, info)?;

    renderer.line(MessageStyle::Info, "")?;

    // Render MCP tools section if any
    if !info.mcp_tools.is_empty() {
        renderer.line(MessageStyle::Info, "MCP tools · /mcp")?;
        for tool in &info.mcp_tools {
            renderer.line(
                MessageStyle::Output,
                &format!("└ {}: {} tokens", tool.name, format_tokens(tool.tokens)),
            )?;
        }
        renderer.line(MessageStyle::Info, "")?;
    }

    // Render custom agents section if any
    if !info.custom_agents.is_empty() {
        renderer.line(MessageStyle::Info, "Custom agents · /agents")?;
        renderer.line(MessageStyle::Info, "")?;
        renderer.line(MessageStyle::Info, "Project")?;
        for agent in &info.custom_agents {
            renderer.line(
                MessageStyle::Output,
                &format!("└ {}: {} tokens", agent.name, format_tokens(agent.tokens)),
            )?;
        }
        renderer.line(MessageStyle::Info, "")?;
    }

    // Render memory files section if any
    if !info.memory_files.is_empty() {
        renderer.line(MessageStyle::Info, "Memory files · /memory")?;
        for file in &info.memory_files {
            renderer.line(
                MessageStyle::Output,
                &format!("└ {}: {} tokens", file.name, format_tokens(file.tokens)),
            )?;
        }
        renderer.line(MessageStyle::Info, "")?;
    }

    // Render skills and slash commands section
    renderer.line(MessageStyle::Info, "Skills and slash commands · /skills")?;
    renderer.line(MessageStyle::Info, "")?;

    // User skills
    if !info.user_skills.is_empty() {
        renderer.line(MessageStyle::Info, "User")?;
        for skill in &info.user_skills {
            renderer.line(
                MessageStyle::Output,
                &format!("└ {}: {} tokens", skill.name, format_tokens(skill.tokens)),
            )?;
        }
        renderer.line(MessageStyle::Info, "")?;
    }

    // Project skills
    if !info.project_skills.is_empty() {
        renderer.line(MessageStyle::Info, "Project")?;
        for skill in &info.project_skills {
            renderer.line(
                MessageStyle::Output,
                &format!("└ {}: {} tokens", skill.name, format_tokens(skill.tokens)),
            )?;
        }
        renderer.line(MessageStyle::Info, "")?;
    }

    // Show slash commands summary
    let slash_count = SLASH_COMMANDS.len();
    renderer.line(
        MessageStyle::Output,
        &format!("└ {} slash commands available (/help)", slash_count),
    )?;

    Ok(())
}

/// Render the visual token bar (10 columns x variable rows)
fn render_token_bar(renderer: &mut AnsiRenderer, info: &ContextUsageInfo) -> Result<()> {
    let total_cells: usize = 100; // 10 columns x 10 rows
    let usage_percent = info.usage_percent();
    let buffer_percent = info.autocompact_buffer_percent;
    let free_percent = 100.0 - usage_percent - buffer_percent;

    // Calculate cells for each type
    let used_cells = ((usage_percent / 100.0) * total_cells as f64).round() as usize;
    let buffer_cells = ((buffer_percent / 100.0) * total_cells as f64).round() as usize;
    let free_cells = total_cells.saturating_sub(used_cells).saturating_sub(buffer_cells);

    // Build the bar symbols
    let mut symbols: Vec<char> = Vec::with_capacity(total_cells);
    symbols.extend(std::iter::repeat_n(SYMBOL_FILLED, used_cells));
    symbols.extend(std::iter::repeat_n(SYMBOL_FREE, free_cells));
    symbols.extend(std::iter::repeat_n(SYMBOL_BUFFER, buffer_cells));

    // Ensure we have exactly 100 symbols
    symbols.resize(total_cells, SYMBOL_FREE);

    // Render first row with model info
    let row1: String = symbols[0..10].iter().collect();
    let model_info = format!(
        "{} · {}/{} tokens ({:.0}%)",
        info.model_name,
        format_tokens(info.current_tokens),
        format_tokens(info.max_tokens),
        usage_percent
    );
    renderer.line(MessageStyle::Info, &format!("{row1}   {model_info}"))?;

    // Render second row
    let row2: String = symbols[10..20].iter().collect();
    renderer.line(MessageStyle::Info, &row2)?;

    // Render remaining rows with breakdown info
    let breakdowns = [
        (info.system_prompt_tokens, "System prompt"),
        (info.system_tools_tokens, "System tools"),
        (info.mcp_tools_tokens, "MCP tools"),
        (info.custom_agents_tokens, "Custom agents"),
        (info.memory_files_tokens, "Memory files"),
        (info.messages_tokens, "Messages"),
    ];

    for (row_idx, (tokens, label)) in breakdowns.iter().enumerate() {
        let start = (row_idx + 2) * 10;
        let end = start + 10;
        if end <= symbols.len() {
            let row: String = symbols[start..end].iter().collect();
            if *tokens > 0 {
                let percent = (*tokens as f64 / info.max_tokens as f64) * 100.0;
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "{}   {} {}: {} tokens ({:.1}%)",
                        row,
                        SYMBOL_FILLED,
                        label,
                        format_tokens(*tokens),
                        percent
                    ),
                )?;
            } else {
                renderer.line(MessageStyle::Info, &row)?;
            }
        }
    }

    // Render free space and buffer info
    let free_tokens = info.free_tokens();
    let buffer_tokens = ((info.autocompact_buffer_percent / 100.0) * info.max_tokens as f64) as usize;

    let row8: String = symbols[80..90].iter().collect();
    renderer.line(
        MessageStyle::Info,
        &format!(
            "{}   {} Free space: {} ({:.1}%)",
            row8,
            SYMBOL_FREE,
            format_tokens(free_tokens),
            free_percent
        ),
    )?;

    let row9: String = symbols[90..100].iter().collect();
    renderer.line(
        MessageStyle::Info,
        &format!(
            "{}   {} Autocompact buffer: {} tokens ({:.1}%)",
            row9,
            SYMBOL_BUFFER,
            format_tokens(buffer_tokens),
            buffer_percent
        ),
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1000), "1.0k");
        assert_eq!(format_tokens(1500), "1.5k");
        assert_eq!(format_tokens(12345), "12.3k");
    }

    #[test]
    fn test_context_usage_info_usage_percent() {
        let mut info = ContextUsageInfo::new("test-model", 200000);
        info.current_tokens = 76000;
        assert!((info.usage_percent() - 38.0).abs() < 0.1);
    }

    #[test]
    fn test_context_item_new() {
        let item = ContextItem::new("test-skill", 1234);
        assert_eq!(item.name, "test-skill");
        assert_eq!(item.tokens, 1234);
    }
}
