//! Dual-channel tool execution helpers.

use anyhow::Result;
use serde_json::Value;
use tracing::{debug, warn};

use crate::config::constants::tools;
use crate::tools::summarizers::{
    Summarizer,
    execution::BashSummarizer,
    file_ops::{EditSummarizer, ReadSummarizer},
    search::{GrepSummarizer, ListSummarizer},
};

use super::{SplitToolResult, ToolRegistry};

impl ToolRegistry {
    /// Execute tool with dual-channel output (Phase 4: Split Tool Results).
    ///
    /// This method enables significant token savings by separating:
    /// - `llm_content`: Concise summary sent to LLM context (token-optimized)
    /// - `ui_content`: Rich output displayed to user (full details)
    ///
    /// For tools with registered summarizers, this can achieve 90-97% token reduction
    /// on tool outputs while preserving full details for the UI.
    ///
    /// # Example
    /// ```rust,no_run
    /// let result = registry.execute_tool_dual("grep_file", args).await?;
    /// // result.llm_content: "Found 127 matches in 15 files. Key: src/tools/grep.rs (3)"
    /// // result.ui_content: [Full formatted output with all 127 matches]
    /// // Savings: ~98% token reduction
    /// ```
    pub async fn execute_tool_dual(&self, name: &str, args: Value) -> Result<SplitToolResult> {
        // Execute the tool using existing infrastructure
        let result = self.execute_tool_ref(name, &args).await?;

        // Convert Value to string for UI content
        let ui_content = if result.is_string() {
            result.as_str().unwrap_or("").to_string()
        } else {
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())
        };

        // Get canonical tool name for summarizer lookup
        // Resolve alias through registration lookup first
        let tool_name = if let Some(registration) = self.inventory.registration_for(name) {
            registration.name()
        } else {
            name // Fallback to original name if not found
        };

        // Check if we have a summarizer for this tool
        match tool_name {
            tools::GREP_FILE => {
                // Apply grep summarization
                let summarizer = GrepSummarizer::default();
                match summarizer.summarize(&ui_content, None) {
                    Ok(llm_content) => {
                        debug!(
                            tool = tools::GREP_FILE,
                            ui_tokens = %summarizer.estimate_savings(&ui_content, &llm_content).1,
                            llm_tokens = %summarizer.estimate_savings(&ui_content, &llm_content).0,
                            savings_pct = %summarizer.estimate_savings(&ui_content, &llm_content).2,
                            "Applied grep summarization"
                        );
                        Ok(SplitToolResult::new(tool_name, llm_content, ui_content))
                    }
                    Err(e) => {
                        warn!(
                            tool = tools::GREP_FILE,
                            error = %e,
                            "Failed to summarize grep output, using simple result"
                        );
                        Ok(SplitToolResult::simple(tool_name, ui_content))
                    }
                }
            }
            tools::LIST_FILES => {
                // Apply list summarization
                let summarizer = ListSummarizer::default();
                match summarizer.summarize(&ui_content, None) {
                    Ok(llm_content) => {
                        debug!(
                            tool = tools::LIST_FILES,
                            ui_tokens = %summarizer.estimate_savings(&ui_content, &llm_content).1,
                            llm_tokens = %summarizer.estimate_savings(&ui_content, &llm_content).0,
                            savings_pct = %summarizer.estimate_savings(&ui_content, &llm_content).2,
                            "Applied list summarization"
                        );
                        Ok(SplitToolResult::new(tool_name, llm_content, ui_content))
                    }
                    Err(e) => {
                        warn!(
                            tool = tools::LIST_FILES,
                            error = %e,
                            "Failed to summarize list output, using simple result"
                        );
                        Ok(SplitToolResult::simple(tool_name, ui_content))
                    }
                }
            }
            tools::READ_FILE => {
                // Apply read file summarization
                let summarizer = ReadSummarizer::default();
                // Extract file path from args if available for better summary
                let metadata = args.as_object().map(|_| args.clone());
                match summarizer.summarize(&ui_content, metadata.as_ref()) {
                    Ok(llm_content) => {
                        debug!(
                            tool = tools::READ_FILE,
                            ui_tokens = %summarizer.estimate_savings(&ui_content, &llm_content).1,
                            llm_tokens = %summarizer.estimate_savings(&ui_content, &llm_content).0,
                            savings_pct = %summarizer.estimate_savings(&ui_content, &llm_content).2,
                            "Applied read file summarization"
                        );
                        Ok(SplitToolResult::new(tool_name, llm_content, ui_content))
                    }
                    Err(e) => {
                        warn!(
                            tool = tools::READ_FILE,
                            error = %e,
                            "Failed to summarize read output, using simple result"
                        );
                        Ok(SplitToolResult::simple(tool_name, ui_content))
                    }
                }
            }
            tools::RUN_PTY_CMD => {
                // Apply bash execution summarization
                let summarizer = BashSummarizer::default();
                // Pass command info from args if available
                let metadata = args.as_object().map(|_| args.clone());
                match summarizer.summarize(&ui_content, metadata.as_ref()) {
                    Ok(llm_content) => {
                        debug!(
                            tool = tools::RUN_PTY_CMD,
                            ui_tokens = %summarizer.estimate_savings(&ui_content, &llm_content).1,
                            llm_tokens = %summarizer.estimate_savings(&ui_content, &llm_content).0,
                            savings_pct = %summarizer.estimate_savings(&ui_content, &llm_content).2,
                            "Applied bash summarization"
                        );
                        Ok(SplitToolResult::new(tool_name, llm_content, ui_content))
                    }
                    Err(e) => {
                        warn!(
                            tool = tools::RUN_PTY_CMD,
                            error = %e,
                            "Failed to summarize bash output, using simple result"
                        );
                        Ok(SplitToolResult::simple(tool_name, ui_content))
                    }
                }
            }
            tools::WRITE_FILE | tools::EDIT_FILE | tools::APPLY_PATCH => {
                // Apply edit/write file summarization
                let summarizer = EditSummarizer::default();
                match summarizer.summarize(&ui_content, None) {
                    Ok(llm_content) => {
                        debug!(
                            tool = tool_name,
                            ui_tokens = %summarizer.estimate_savings(&ui_content, &llm_content).1,
                            llm_tokens = %summarizer.estimate_savings(&ui_content, &llm_content).0,
                            savings_pct = %summarizer.estimate_savings(&ui_content, &llm_content).2,
                            "Applied edit summarization"
                        );
                        Ok(SplitToolResult::new(tool_name, llm_content, ui_content))
                    }
                    Err(e) => {
                        warn!(
                            tool = tool_name,
                            error = %e,
                            "Failed to summarize edit output, using simple result"
                        );
                        Ok(SplitToolResult::simple(tool_name, ui_content))
                    }
                }
            }
            _ => {
                // No summarizer registered, use same content for both channels
                Ok(SplitToolResult::simple(tool_name, ui_content))
            }
        }
    }
}
