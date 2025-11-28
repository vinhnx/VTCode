//! LLM error display utilities with enhanced ANSI color support
//!
//! This module provides enhanced error display capabilities for LLM providers
//! using standard console styling for consistent terminal output.

use crate::ui::styled::Styles;
use anstyle::Style;

/// Internal helper to wrap text with style codes - reduces duplication
#[inline]
fn style_text(style: Style, text: &str) -> String {
    format!(
        "{}{}{}",
        Styles::render(&style),
        text,
        Styles::render_reset()
    )
}

/// Get a styled error message with enhanced coloring
#[inline]
pub fn style_llm_error(message: &str) -> String {
    style_text(Styles::error(), message)
}

/// Get a styled warning message with enhanced coloring
#[inline]
pub fn style_llm_warning(message: &str) -> String {
    style_text(Styles::warning(), message)
}

/// Get a styled success message with enhanced coloring
#[inline]
pub fn style_llm_success(message: &str) -> String {
    style_text(Styles::success(), message)
}

/// Get a styled provider name with enhanced coloring based on provider type
pub fn style_provider_name(provider: &str) -> String {
    let style = match provider.to_lowercase().as_str() {
        "gemini" => Styles::info(),    // Deep blue for Gemini
        "openai" => Styles::warning(), // Bright orange for OpenAI
        "anthropic" => Styles::code(), // Anthropic's brand purple
        _ => Styles::debug(),          // Default styling for other providers
    };
    style_text(style, provider)
}

/// Format an LLM error for display with enhanced coloring
pub fn format_llm_error(provider: &str, error: &str) -> String {
    let provider_styled = style_provider_name(provider);
    let error_styled = style_llm_error(error);
    format!("{} {}", provider_styled, error_styled)
}

/// Format an LLM warning for display with enhanced coloring
pub fn format_llm_warning(provider: &str, warning: &str) -> String {
    let provider_styled = style_provider_name(provider);
    let warning_styled = style_llm_warning(warning);
    format!("{} {}", provider_styled, warning_styled)
}

/// Format an LLM success message for display with enhanced coloring
pub fn format_llm_success(provider: &str, message: &str) -> String {
    let provider_styled = style_provider_name(provider);
    let success_styled = style_llm_success(message);
    format!("{} {}", provider_styled, success_styled)
}

/// Format a network error for display with enhanced coloring.
/// This is a convenience wrapper for the common "Network error: {}" pattern.
#[inline]
pub fn format_network_error(provider: &str, error: &impl std::fmt::Display) -> String {
    format_llm_error(provider, &format!("Network error: {}", error))
}

/// Format a parse error for display with enhanced coloring.
/// This is a convenience wrapper for the common "Parse error: {}" pattern.
#[inline]
pub fn format_parse_error(provider: &str, error: &impl std::fmt::Display) -> String {
    format_llm_error(provider, &format!("Parse error: {}", error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_style_llm_error() {
        let result = style_llm_error("Test error");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_style_llm_warning() {
        let result = style_llm_warning("Test warning");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_style_llm_success() {
        let result = style_llm_success("Test success");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_style_provider_name() {
        let providers = vec![
            "gemini",
            "openai",
            "anthropic",
            "ollama",
            "lmstudio",
            "unknown",
        ];
        for provider in providers {
            let result = style_provider_name(provider);
            assert!(!result.is_empty());
        }
    }

    #[test]
    fn test_format_llm_error() {
        let result = format_llm_error("gemini", "Connection failed");
        assert!(result.contains("gemini"));
        assert!(result.contains("Connection failed"));
    }

    #[test]
    fn test_format_llm_warning() {
        let result = format_llm_warning("openai", "Rate limit approaching");
        assert!(result.contains("openai"));
        assert!(result.contains("Rate limit approaching"));
    }

    #[test]
    fn test_format_llm_success() {
        let result = format_llm_success("anthropic", "Request completed");
        assert!(result.contains("anthropic"));
        assert!(result.contains("Request completed"));
    }
}
