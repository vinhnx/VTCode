//! Provider tool formatters.
//!
//! Each provider (or family) shapes `ToolDefinition` into the JSON it expects on the
//! wire. Historically, this logic was scattered across the request builders of each
//! provider crate, making it hard to verify cross-provider invariants (e.g. that
//! `defer_loading`, `strict`, `input_examples`, `allowed_callers` are preserved when
//! they apply). This module introduces a single trait that all providers can
//! implement, plus ready-made implementations for the most common cases.
//!
//! The trait is intentionally additive: existing callers can keep using the
//! per-provider helpers (e.g. `serialize_tools_openai_format`,
//! `anthropic::request_builder::tools::build_tools`). New callers should reach for
//! `formatter_for(provider_id, model)` instead.
//!
//! See *The Hitchhiker's Guide to Agentic AI* §18.4.1 for the underlying model
//! (separate OpenAI / Anthropic / Gemini / MCP wire shapes) and §18.4.2 for the
//! selection / routing concerns that sit above this layer.

use serde_json::Value;

use crate::provider::ToolDefinition;

/// Identifies a provider family for the purpose of tool formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderFamily {
    /// OpenAI Chat Completions API (function-calling shape).
    OpenAIChat,
    /// OpenAI Responses API (hosted tools, apply_patch, shell, custom, grammar).
    OpenAIResponses,
    /// Anthropic Messages API (input_schema, native hosted tools, advanced-tool-use).
    Anthropic,
    /// Google Gemini generateContent API (function declarations + native hosted tools).
    Gemini,
    /// Generic OpenAI-compatible Chat Completions API (DeepSeek, ZAI, Moonshot, …).
    OpenAICompatible,
}

impl ProviderFamily {
    /// Resolve a provider family from a canonical provider identifier (e.g. "openai",
    /// "anthropic", "gemini", "deepseek"). Falls back to `OpenAICompatible` for any
    /// unknown identifier — most providers in this class expose the same function
    /// calling shape.
    #[must_use]
    pub fn from_provider_id(provider_id: &str) -> Self {
        match provider_id.to_ascii_lowercase().as_str() {
            "openai" => Self::OpenAIChat,
            "openai-responses" | "openai_responses" => Self::OpenAIResponses,
            "anthropic" | "claude" => Self::Anthropic,
            "gemini" | "google" | "google-gemini" => Self::Gemini,
            _ => Self::OpenAICompatible,
        }
    }
}

/// Trait implemented by every provider tool formatter.
///
/// Implementations are expected to be stateless. Constructors live in
/// [`formatters`] below.
pub trait ProviderToolFormatter: Send + Sync {
    /// Family this formatter belongs to (for diagnostics).
    fn family(&self) -> ProviderFamily;

    /// Provider extensions preserved by this formatter. Used by callers that want to
    /// validate that a particular extension (e.g. `defer_loading`) is supported before
    /// passing a tool through.
    fn supported_extensions(&self) -> &'static [&'static str];

    /// Returns true when this formatter accepts the given tool type unchanged.
    fn supports(&self, tool: &ToolDefinition) -> bool;

    /// Format a slice of tool definitions into the wire-shape this provider expects.
    ///
    /// Returns `None` when the slice is empty (mirrors the existing per-provider
    /// helpers).
    fn format_tools(&self, tools: &[ToolDefinition], model: &str) -> Option<Value>;
}

/// Anthropic formatter — extracts logic from
/// `vtcode-llm/src/providers/anthropic/request_builder/tools.rs::build_tools`.
pub struct AnthropicFormatter;

impl ProviderToolFormatter for AnthropicFormatter {
    fn family(&self) -> ProviderFamily {
        ProviderFamily::Anthropic
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        &[
            "input_examples",
            "strict",
            "allowed_callers",
            "defer_loading",
            "web_search_options",
            "tool_search",
            "code_execution",
            "memory",
        ]
    }

    fn supports(&self, tool: &ToolDefinition) -> bool {
        tool.is_tool_search()
            || tool.is_anthropic_web_search()
            || tool.is_anthropic_code_execution()
            || tool.is_anthropic_memory_tool()
            || tool.function.is_some()
    }

    fn format_tools(&self, tools: &[ToolDefinition], _model: &str) -> Option<Value> {
        // Delegate to the existing build helper so we never regress the wire shape.
        // The function is wired through `crate::providers::anthropic::request_builder::tools`
        // to keep Anthropic-specific knowledge in one place.
        super::anthropic::request_builder::tools::build_tools_via_formatter(tools)
    }
}

/// OpenAI Chat Completions formatter — extracts logic from
/// `vtcode-llm/src/providers/common.rs::serialize_tools_openai_format`.
pub struct OpenAIChatFormatter;

impl ProviderToolFormatter for OpenAIChatFormatter {
    fn family(&self) -> ProviderFamily {
        ProviderFamily::OpenAIChat
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        &["function_only"]
    }

    fn supports(&self, tool: &ToolDefinition) -> bool {
        tool.tool_type == "function" || tool.tool_type == "web_search"
    }

    fn format_tools(&self, tools: &[ToolDefinition], _model: &str) -> Option<Value> {
        super::common::serialize_tools_openai_format(tools).map(Value::Array)
    }
}

/// OpenAI Responses API formatter — model-aware, preserves `defer_loading`.
pub struct OpenAIResponsesFormatter;

impl ProviderToolFormatter for OpenAIResponsesFormatter {
    fn family(&self) -> ProviderFamily {
        ProviderFamily::OpenAIResponses
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        &[
            "defer_loading",
            "shell",
            "apply_patch",
            "custom",
            "grammar",
            "tool_search",
            "hosted_web_search",
            "hosted_file_search",
            "hosted_mcp",
        ]
    }

    fn supports(&self, _tool: &ToolDefinition) -> bool {
        // The Responses API explicitly handles every variant of `ToolDefinition` —
        // there's no tool type that it rejects outright.
        true
    }

    fn format_tools(&self, tools: &[ToolDefinition], _model: &str) -> Option<Value> {
        // Defer to the existing per-provider helper to keep the wire shape stable.
        super::openai::tool_serialization::serialize_tools_for_responses(tools, None)
    }
}

/// Gemini formatter — uses `function_declarations` plus native hosted tool shapes.
pub struct GeminiFormatter;

impl ProviderToolFormatter for GeminiFormatter {
    fn family(&self) -> ProviderFamily {
        ProviderFamily::Gemini
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        &[
            "google_search",
            "google_maps",
            "url_context",
            "code_execution",
            "function_declarations",
        ]
    }

    fn supports(&self, tool: &ToolDefinition) -> bool {
        matches!(
            tool.tool_type.as_str(),
            "function" | "google_search" | "google_maps" | "url_context" | "code_execution"
        ) || tool.function.is_some()
    }

    fn format_tools(&self, tools: &[ToolDefinition], _model: &str) -> Option<Value> {
        // Delegate to the existing helper so the wire shape stays in lockstep with
        // what the Gemini request builder already does today.
        super::gemini::helpers::serialize_gemini_tools(tools)
    }
}

/// Generic OpenAI-compatible formatter. Unlike `OpenAIChatFormatter`, this one is
/// intentionally conservative: it only emits function-shaped tools and silently drops
/// hosted / native tool types because most compatible providers don't support them.
pub struct OpenAICompatibleFormatter;

impl ProviderToolFormatter for OpenAICompatibleFormatter {
    fn family(&self) -> ProviderFamily {
        ProviderFamily::OpenAICompatible
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        &["function_only"]
    }

    fn supports(&self, tool: &ToolDefinition) -> bool {
        tool.tool_type == "function" || tool.tool_type == "web_search"
    }

    fn format_tools(&self, tools: &[ToolDefinition], _model: &str) -> Option<Value> {
        super::common::serialize_tools_openai_format(tools).map(Value::Array)
    }
}

/// Build a formatter for a given provider family.
#[must_use]
pub fn formatter_for_family(family: ProviderFamily) -> Box<dyn ProviderToolFormatter> {
    match family {
        ProviderFamily::OpenAIChat => Box::new(OpenAIChatFormatter),
        ProviderFamily::OpenAIResponses => Box::new(OpenAIResponsesFormatter),
        ProviderFamily::Anthropic => Box::new(AnthropicFormatter),
        ProviderFamily::Gemini => Box::new(GeminiFormatter),
        ProviderFamily::OpenAICompatible => Box::new(OpenAICompatibleFormatter),
    }
}

/// Convenience: resolve a formatter from a provider identifier. This is the entry
/// point the runloop should use when constructing an LLM request.
#[must_use]
pub fn formatter_for_provider(provider_id: &str) -> Box<dyn ProviderToolFormatter> {
    formatter_for_family(ProviderFamily::from_provider_id(provider_id))
}

/// Convenience: resolve a formatter using the explicit `ProviderFamily`. Useful in
/// tests or when the runloop already knows the family without parsing the provider
/// string.
#[must_use]
pub fn formatter_for(family: ProviderFamily) -> Box<dyn ProviderToolFormatter> {
    formatter_for_family(family)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_function_tool() -> ToolDefinition {
        ToolDefinition::function(
            "search_docs".to_owned(),
            "Search documentation".to_owned(),
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }),
        )
    }

    #[test]
    fn provider_family_resolution_handles_known_ids() {
        assert_eq!(
            ProviderFamily::from_provider_id("openai"),
            ProviderFamily::OpenAIChat
        );
        assert_eq!(
            ProviderFamily::from_provider_id("anthropic"),
            ProviderFamily::Anthropic
        );
        assert_eq!(
            ProviderFamily::from_provider_id("gemini"),
            ProviderFamily::Gemini
        );
        assert_eq!(
            ProviderFamily::from_provider_id("deepseek"),
            ProviderFamily::OpenAICompatible
        );
        assert_eq!(
            ProviderFamily::from_provider_id("unknown"),
            ProviderFamily::OpenAICompatible
        );
    }

    #[test]
    fn formatter_for_provider_returns_trait_object() {
        let f = formatter_for_provider("anthropic");
        assert_eq!(f.family(), ProviderFamily::Anthropic);

        let f = formatter_for_provider("openai");
        assert_eq!(f.family(), ProviderFamily::OpenAIChat);

        let f = formatter_for_provider("deepseek");
        assert_eq!(f.family(), ProviderFamily::OpenAICompatible);
    }

    #[test]
    fn empty_tool_slice_formats_to_none() {
        // The formatter contract mirrors existing helpers: an empty tool slice yields
        // `None` so callers can omit the `"tools"` key from the wire payload.
        let f = formatter_for_provider("anthropic");
        assert!(f.format_tools(&[], "claude-opus-4-7").is_none());

        let f = formatter_for_provider("deepseek");
        assert!(f.format_tools(&[], "deepseek-chat").is_none());
    }

    #[test]
    fn openai_compatible_formatter_silently_drops_extensions() {
        // Build a tool with `defer_loading` and `strict` set; the OpenAI-compatible
        // formatter must drop both (the serializer has nowhere to put them).
        let tool = sample_function_tool()
            .with_strict(true)
            .with_defer_loading(true);

        let f = formatter_for_provider("deepseek");
        let value = f
            .format_tools(std::slice::from_ref(&tool), "deepseek-chat")
            .expect("formatter should yield a value for a non-empty slice");
        let arr = value.as_array().expect("expected array");
        assert_eq!(arr.len(), 1);
        let serialized = &arr[0];
        assert!(
            serialized.get("defer_loading").is_none(),
            "openai-compatible formatter must drop defer_loading"
        );
        assert!(
            serialized.get("strict").is_none(),
            "openai-compatible formatter must drop strict"
        );
    }

    #[test]
    fn anthropic_formatter_preserves_function_extension_fields() {
        // `strict` and `input_examples` are explicitly preserved by the Anthropic
        // path; this guards against a regression that drops them on the floor.
        let tool = sample_function_tool()
            .with_strict(true)
            .with_input_examples(vec![json!({
                "input": "Find Rust docs",
                "tool_use": { "query": "rust" }
            })]);

        let f = formatter_for_provider("anthropic");
        assert!(f.supports(&tool));

        let value = f
            .format_tools(std::slice::from_ref(&tool), "claude-opus-4-7")
            .expect("formatter should yield a value");
        let serialized = value.to_string();
        assert!(
            serialized.contains("strict"),
            "anthropic wire payload missing strict: {serialized}"
        );
        assert!(
            serialized.contains("input_examples"),
            "anthropic wire payload missing input_examples: {serialized}"
        );
    }

    #[test]
    fn formatter_extensions_are_non_empty_for_every_family() {
        // Each family exposes a non-empty extension list so callers can introspect
        // what they support before emitting a request.
        for family in [
            ProviderFamily::OpenAIChat,
            ProviderFamily::OpenAIResponses,
            ProviderFamily::Anthropic,
            ProviderFamily::Gemini,
            ProviderFamily::OpenAICompatible,
        ] {
            let f = formatter_for_family(family);
            assert!(
                !f.supported_extensions().is_empty(),
                "{family:?} must report at least one extension"
            );
        }
    }
}
