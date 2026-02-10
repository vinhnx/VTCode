//! Execution result summarization
//!
//! Summarizes bash and code execution outputs from full stdout/stderr
//! into concise summaries suitable for LLM context.
//!
//! ## Strategy
//!
//! Instead of sending full command output (potentially megabytes),
//! send essential information:
//! - Command executed
//! - Exit code (success/failure)
//! - First N lines of output
//! - Last N lines of output
//! - Total output size indicator
//!
//! Target: ~150-250 tokens vs potentially thousands

use super::{Summarizer, truncate_line, truncate_to_tokens};
use anyhow::Result;
use serde_json::Value;

/// Summarizer for bash/shell execution results
pub struct BashSummarizer {
    /// Maximum lines to show from start of output
    pub max_head_lines: usize,
    /// Maximum lines to show from end of output
    pub max_tail_lines: usize,
    /// Maximum tokens for entire summary
    pub max_tokens: usize,
}

impl Default for BashSummarizer {
    fn default() -> Self {
        Self {
            max_head_lines: 20,
            max_tail_lines: 10,
            max_tokens: 500, // ~1000 chars for token efficiency
        }
    }
}

impl Summarizer for BashSummarizer {
    fn summarize(&self, full_output: &str, metadata: Option<&Value>) -> Result<String> {
        // Parse execution result
        let result = parse_bash_output(full_output, metadata);

        // Build summary
        let mut summary = String::new();

        // Command header
        if let Some(cmd) = result.command {
            summary.push_str(&format!("Command: {}\n", truncate_command(&cmd, 100)));
        }

        // Exit status
        summary.push_str(&format!(
            "Exit code: {} ({})\n",
            result.exit_code,
            if result.success { "success" } else { "failed" }
        ));

        // Execution time if available
        if let Some(duration_ms) = result.duration_ms {
            if duration_ms > 1000 {
                summary.push_str(&format!("Duration: {:.1}s\n", duration_ms as f64 / 1000.0));
            } else {
                summary.push_str(&format!("Duration: {}ms\n", duration_ms));
            }
        }

        // Output summary
        if result.total_lines > 0 {
            summary.push_str(&format!("\nOutput: {} lines", result.total_lines));

            if result.total_bytes > 10_000 {
                let kb = result.total_bytes / 1024;
                summary.push_str(&format!(" ({} KB)", kb));
            }

            summary.push('\n');

            // Head lines
            if !result.head_lines.is_empty() {
                summary.push_str("\nFirst lines:\n");
                for line in &result.head_lines {
                    summary.push_str(&truncate_line(line, 120));
                    summary.push('\n');
                }

                if result.total_lines > self.max_head_lines {
                    let omitted = result
                        .total_lines
                        .saturating_sub(self.max_head_lines + self.max_tail_lines);
                    if omitted > 0 {
                        summary.push_str(&format!("[...{} more lines]\n", omitted));
                    }
                }
            }

            // Tail lines for long output
            if result.total_lines > self.max_head_lines + 1 && !result.tail_lines.is_empty() {
                summary.push_str("\nLast lines:\n");
                for line in &result.tail_lines {
                    summary.push_str(&truncate_line(line, 120));
                    summary.push('\n');
                }
            }
        } else if !result.stderr.is_empty() {
            // Show stderr if no stdout
            summary.push_str("\nError output:\n");
            for line in result.stderr.lines().take(self.max_head_lines) {
                summary.push_str(&truncate_line(line, 120));
                summary.push('\n');
            }
        } else {
            summary.push_str("\n(No output)\n");
        }

        Ok(truncate_to_tokens(&summary, self.max_tokens))
    }
}

/// Execution result statistics
#[derive(Debug, Default)]
struct BashResult {
    command: Option<String>,
    exit_code: i32,
    success: bool,
    duration_ms: Option<u64>,
    total_lines: usize,
    total_bytes: usize,
    head_lines: Vec<String>,
    tail_lines: Vec<String>,
    stderr: String,
}

/// Parse bash execution output
fn parse_bash_output(output: &str, metadata: Option<&Value>) -> BashResult {
    let mut result = BashResult::default();

    // Try to parse as JSON first (structured output from bash tool)
    if let Ok(json) = serde_json::from_str::<Value>(output) {
        result.command = json
            .get("command")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        result.exit_code = json
            .get("exit_code")
            .or_else(|| json.get("exitCode"))
            .and_then(|e| e.as_i64())
            .unwrap_or(0) as i32;

        result.success = json
            .get("success")
            .and_then(|s| s.as_bool())
            .unwrap_or(result.exit_code == 0);

        result.duration_ms = json
            .get("duration_ms")
            .or_else(|| json.get("durationMs"))
            .and_then(|d| d.as_u64());

        // Extract stdout
        if let Some(stdout) = json.get("stdout").or_else(|| json.get("output")) {
            let stdout_str = if let Some(s) = stdout.as_str() {
                s
            } else {
                &serde_json::to_string_pretty(stdout).unwrap_or_default()
            };

            parse_output_lines(stdout_str, &mut result);
        }

        // Extract stderr
        if let Some(stderr) = json.get("stderr").or_else(|| json.get("error")) {
            result.stderr = stderr.as_str().unwrap_or("").to_string();
        }
    } else {
        // Fallback: treat entire output as stdout
        parse_output_lines(output, &mut result);
        result.success = !output.to_lowercase().contains("error");

        // Extract command from metadata if available
        if let Some(meta) = metadata {
            result.command = meta
                .get("command")
                .and_then(|c| c.as_str())
                .map(|s| s.to_string());
        }
    }

    result
}

/// Parse output text into head/tail lines
fn parse_output_lines(output: &str, result: &mut BashResult) {
    let lines: Vec<&str> = output.lines().collect();
    result.total_lines = lines.len();
    result.total_bytes = output.len();

    // Head lines (first 5)
    result.head_lines = lines.iter().take(5).map(|line| line.to_string()).collect();

    // Tail lines (last 3) if output is long
    if lines.len() > 8 {
        result.tail_lines = lines
            .iter()
            .rev()
            .take(3)
            .rev()
            .map(|line| line.to_string())
            .collect();
    }
}

/// Truncate command string to max length
fn truncate_command(cmd: &str, max_len: usize) -> String {
    if cmd.len() <= max_len {
        cmd.to_string()
    } else {
        let target = max_len.saturating_sub(3);
        let end = cmd
            .char_indices()
            .map(|(i, _)| i)
            .rfind(|&i| i <= target)
            .unwrap_or(0);
        format!("{}...", &cmd[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bash_summarizer_json_success() {
        let full_output = r#"{
            "command": "ls -la /tmp",
            "exit_code": 0,
            "success": true,
            "duration_ms": 42,
            "stdout": "total 100\ndrwx------  5 user  wheel  160 Dec 21 10:30 .\ndrwxr-xr-x  6 root  wheel  192 Dec 20 08:00 ..\n-rw-r--r--  1 user  wheel  512 Dec 21 10:30 file.txt"
        }"#;

        let summarizer = BashSummarizer::default();
        let summary = summarizer.summarize(full_output, None).unwrap();

        assert!(summary.contains("Command: ls -la /tmp"));
        assert!(summary.contains("Exit code: 0 (success)"));
        assert!(summary.contains("Duration: 42ms"));
        assert!(summary.contains("Output: 4 lines"));
        assert!(summary.contains("total 100"));

        // Verify some savings (small test input has lower percentage)
        let (_llm, _ui, pct) = summarizer.estimate_savings(full_output, &summary);
        assert!(pct > 15.0, "Should save >15% (got {:.1}%)", pct);
    }

    #[test]
    fn test_bash_summarizer_json_failure() {
        let full_output = r#"{
            "command": "cat nonexistent.txt",
            "exit_code": 1,
            "success": false,
            "stderr": "cat: nonexistent.txt: No such file or directory"
        }"#;

        let summarizer = BashSummarizer::default();
        let summary = summarizer.summarize(full_output, None).unwrap();

        assert!(summary.contains("Exit code: 1 (failed)"));
        assert!(summary.contains("cat nonexistent.txt"));
        assert!(summary.contains("Error output:") || summary.contains("No such file"));
    }

    #[test]
    fn test_bash_summarizer_large_output() {
        let mut lines = Vec::new();
        for i in 1..=100 {
            lines.push(format!("Line {}: Some output here", i));
        }
        let stdout = lines.join("\n");

        let full_output = serde_json::json!({
            "command": "generate_output",
            "exit_code": 0,
            "success": true,
            "stdout": stdout
        })
        .to_string();

        let summarizer = BashSummarizer::default();
        let summary = summarizer.summarize(&full_output, None).unwrap();

        assert!(summary.contains("Output: 100 lines"));
        assert!(summary.contains("Line 1:"));
        assert!(summary.contains("more lines"));

        // Should show significant savings on large output
        let (_llm, _ui, pct) = summarizer.estimate_savings(&full_output, &summary);
        assert!(
            pct > 70.0,
            "Should save >70% on large output (got {:.1}%)",
            pct
        );
    }

    #[test]
    fn test_bash_summarizer_plain_text() {
        let full_output = "Hello World\nLine 2\nLine 3";

        let summarizer = BashSummarizer::default();
        let summary = summarizer.summarize(full_output, None).unwrap();

        assert!(summary.contains("Output: 3 lines"));
        assert!(summary.contains("Hello World"));
    }

    #[test]
    fn test_bash_summarizer_with_metadata() {
        let full_output = "Command output here";
        let metadata = serde_json::json!({
            "command": "echo 'test'"
        });

        let summarizer = BashSummarizer::default();
        let summary = summarizer.summarize(full_output, Some(&metadata)).unwrap();

        assert!(summary.contains("echo 'test'"));
    }

    #[test]
    fn test_truncate_command() {
        let long_cmd = "a".repeat(200);
        let truncated = truncate_command(&long_cmd, 50);

        assert!(truncated.len() <= 50);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_parse_bash_output_json() {
        let output = r#"{"command": "test", "exit_code": 0, "stdout": "ok"}"#;
        let result = parse_bash_output(output, None);

        assert_eq!(result.command, Some("test".to_string()));
        assert_eq!(result.exit_code, 0);
        assert!(result.success);
    }

    #[test]
    fn test_parse_output_lines() {
        let mut result = BashResult::default();
        let output = "Line 1\nLine 2\nLine 3";
        parse_output_lines(output, &mut result);

        assert_eq!(result.total_lines, 3);
        assert_eq!(result.head_lines.len(), 3);
        assert_eq!(result.head_lines[0], "Line 1");
    }
}
