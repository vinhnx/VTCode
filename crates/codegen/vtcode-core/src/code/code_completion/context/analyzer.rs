use super::CompletionContext;

/// Context analyzer for understanding code context
pub struct ContextAnalyzer;

impl ContextAnalyzer {
    pub fn new() -> Self {
        Self
    }

    /// Analyze code context at the given position
    pub fn analyze(&mut self, source: &str, line: usize, column: usize) -> CompletionContext {
        let language = self.detect_language(source);
        let prefix = self.extract_prefix(source, line, column);

        let mut context = CompletionContext::new(line, column, prefix, language);
        context.scope = Vec::new();
        context.imports = self.extract_imports(source);
        context.recent_symbols = Vec::new();

        context
    }

    fn detect_language(&self, source: &str) -> String {
        let first_lines: String = source.lines().take(20).collect::<Vec<_>>().join("\n");
        if first_lines.contains("use std::")
            || first_lines.contains("fn ") && first_lines.contains("->")
        {
            "rust".into()
        } else if first_lines.contains("import ") && first_lines.contains("from ")
            || first_lines.contains("def ")
        {
            "python".into()
        } else if first_lines.contains("interface ")
            || first_lines.contains(": string")
            || first_lines.contains(": number")
        {
            "typescript".into()
        } else if first_lines.contains("function ")
            || first_lines.contains("const ")
            || first_lines.contains("require(")
        {
            "javascript".into()
        } else if first_lines.contains("package ") && first_lines.contains("func ") {
            "go".into()
        } else if first_lines.contains("public class ") || first_lines.contains("import java.") {
            "java".into()
        } else if first_lines.starts_with("#!/bin/bash") || first_lines.starts_with("#!/bin/sh") {
            "bash".into()
        } else {
            "rust".into()
        }
    }

    fn extract_prefix(&self, source: &str, line: usize, column: usize) -> String {
        let lines: Vec<&str> = source.lines().collect();
        if line < lines.len() && column <= lines[line].len() {
            lines[line][..column].into()
        } else {
            String::new()
        }
    }

    /// Extract import statements from source code using simple line scanning
    fn extract_imports(&self, source: &str) -> Vec<String> {
        source
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                trimmed.starts_with("use ")
                    || trimmed.starts_with("import ")
                    || trimmed.starts_with("from ")
                    || trimmed.starts_with("require(")
                    || trimmed.starts_with("const ") && trimmed.contains("require(")
            })
            .map(|s| s.to_string())
            .collect()
    }
}

impl Default for ContextAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
