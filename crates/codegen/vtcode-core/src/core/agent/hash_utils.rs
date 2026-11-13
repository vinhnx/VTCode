//! Hash utilities for tool catalog and system prompt caching.
//!
//! Provides hashing functions for tool definitions, system prompts, and
//! low-signal attempt deduplication keys.
//!
//! All hashes are stable across processes (FNV-1a) so cache keys remain
//! comparable after restarts and in persisted harness artifacts.

use std::hash::{Hash, Hasher};

use serde::Serialize;
use serde_json::Value;

use crate::llm::provider::ToolDefinition;

/// Resolved wire capabilities that define a cache-stable request segment.
/// Runtime counters and environment observations deliberately do not belong here.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PromptCapabilityIdentity {
    provider: compact_str::CompactString,
    model: compact_str::CompactString,
    context_window: usize,
    reasoning_tag: compact_str::CompactString,
    reasoning_effort: bool,
    parallel_tools: bool,
    tools: bool,
    caching: bool,
    catalog_epoch: u64,
}

impl PromptCapabilityIdentity {
    /// Construct the cache identity available from the generated model catalog.
    ///
    /// The runtime provider path should prefer [`Self::resolve`], because it
    /// can observe provider-specific capabilities such as parallel tool
    /// configuration. Static prompt assembly does not own a provider object,
    /// so it uses this catalog-only identity and keeps missing capability bits
    /// explicit rather than falling back to a model-name hash.
    #[must_use]
    pub fn from_catalog(
        provider: &str,
        model: &str,
        reasoning: Option<crate::config::types::ReasoningEffortLevel>,
        catalog_epoch: u64,
        catalog: Option<vtcode_config::models::ModelCatalogEntry>,
    ) -> Self {
        let context_window = catalog.map(|entry| entry.context_window).unwrap_or_default();
        let reasoning_effort = catalog.is_some_and(|entry| !entry.reasoning_efforts.is_empty());
        let tools = catalog.map(|entry| entry.tool_call).unwrap_or(true);
        let caching = catalog.is_some_and(|entry| entry.caching);
        Self {
            provider: provider.into(),
            model: model.into(),
            context_window,
            reasoning_tag: reasoning.map_or_else(|| "unset".into(), |effort| effort.to_string().into()),
            reasoning_effort,
            // Static prompt assembly has no provider instance from which to
            // resolve this wire capability. Runtime request construction
            // replaces this with the Provider-trait value.
            parallel_tools: false,
            tools,
            caching,
            catalog_epoch,
        }
    }

    /// Prefer discovery/catalog capacity when the caller already resolved a model.
    #[must_use]
    pub fn with_resolved_model(mut self, resolved: &crate::llm::model_resolver::ResolvedModel) -> Self {
        self.context_window = resolved.context_window().unwrap_or(self.context_window);
        self
    }
    #[must_use]
    pub fn resolve(
        provider: &dyn crate::llm::provider::LLMProvider,
        model: &str,
        reasoning: Option<crate::config::types::ReasoningEffortLevel>,
        catalog_epoch: u64,
    ) -> Self {
        Self {
            provider: provider.name().into(),
            model: model.into(),
            context_window: provider.effective_context_size(model),
            reasoning_tag: reasoning.map_or_else(|| "unset".into(), |effort| effort.to_string().into()),
            reasoning_effort: !provider.supported_reasoning_efforts(model).is_empty(),
            parallel_tools: provider.supports_parallel_tool_config(model),
            tools: provider.supports_tools(model),
            caching: provider.supports_context_caching(model),
            catalog_epoch,
        }
    }

    #[must_use]
    pub fn prefix_hash(&self, prompt_hash: u64) -> u64 {
        hash_value(&(prompt_hash, self))
    }

    /// Return the digest for the resolved capabilities alone. Callers that
    /// also have a stable prompt hash can combine the two with
    /// [`Self::prefix_hash`].
    #[must_use]
    pub fn digest(&self) -> u64 {
        hash_value(self)
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn stable_hasher_is_deterministic_across_instances() {
        assert_eq!(hash_value(&"cache-key"), hash_value(&"cache-key"));
        assert_ne!(hash_value(&"cache-key-a"), hash_value(&"cache-key-b"));
    }

    #[test]
    fn stable_prefix_hash_strips_all_runtime_sections() {
        let base = "Base prompt\n[Harness Limits]\n- max_tool_wall_clock_secs: 60";
        for suffix in [
            "\n\n## Active Tools\n- Capabilities: read-only.",
            "\n\n[Runtime Tool Catalog]\n- version: 1",
            "\n\n[Deferred Tools]\n- code_search (2 tools): search",
            "\n\n[Runtime Context]\n- turns: 1",
            "\n\n[History Directives]\n- reuse prior tool outputs",
            "\n\n[Context]\n- workspace: /tmp",
            "\n\n# PLANNING WORKFLOW (READ-ONLY)\nMutating file edits are blocked.",
            "\n\n# FULL-AUTO: Complete task autonomously until done or blocked.\n- Stay within the exposed tool list.",
            "\n\n# FULL-AUTO (PLANNING WORKFLOW): Work autonomously within planning workflow constraints.\n- Stay read-only.",
            "\n\n## Active Primary Agent Runtime State\n- Active agent: Build",
        ] {
            let with_section = format!("{base}{suffix}");
            assert_eq!(
                stable_system_prefix_hash(base),
                stable_system_prefix_hash(&with_section),
                "runtime section should not affect prefix hash: {suffix:?}"
            );
        }
    }

    #[test]
    fn stable_prefix_hash_keeps_stable_content_after_dynamic_appendix() {
        // Static-first/dynamic-last discipline: stable sections after a dynamic
        // appendix must still contribute (explicit classification, not
        // earliest-header truncation). Planning notices themselves stay dynamic.
        let base = "# VT Code\nBase contract";
        let with_planning = format!(
            "{base}\n\n# PLANNING WORKFLOW (READ-ONLY)\nread-only notice\n\n[Harness Limits]\n- max_tool_wall_clock_secs: 60"
        );
        assert_eq!(
            stable_system_prefix_hash(base),
            stable_system_prefix_hash(&with_planning),
            "planning/full-auto notices must not perturb the stable prefix"
        );
    }

    #[test]
    fn stable_prefix_hash_ignores_hash_body_lines_inside_dynamic_section() {
        // Body lines starting with `#` but without a header space (e.g.
        // `#comment`) must not end the dynamic section and leak dynamic
        // content into the stable hash.
        let base = "Base prompt";
        let dynamic = format!("{base}\n\n[Harness Limits]\n- max_tool_wall_clock_secs: 60\n#comment body\n- more");
        assert_eq!(
            stable_system_prefix_hash(base),
            stable_system_prefix_hash(&dynamic),
            "hash-prefixed body lines must not reset the dynamic section"
        );
    }

    #[test]
    fn capability_identity_resolves_three_provider_families_without_transport() {
        use crate::config::constants::models;
        use crate::config::types::ReasoningEffortLevel;
        use crate::llm::provider::LLMProvider;
        use crate::llm::providers::{AnthropicProvider, GeminiProvider, OpenAIProvider};

        let providers: [(Box<dyn LLMProvider>, &str); 3] = [
            (Box::new(OpenAIProvider::new("offline-fixture".into())), models::openai::DEFAULT_MODEL),
            (Box::new(AnthropicProvider::new("offline-fixture".into())), models::anthropic::DEFAULT_MODEL),
            (Box::new(GeminiProvider::new("offline-fixture".into())), models::google::DEFAULT_MODEL),
        ];
        for (provider, model) in providers {
            let identity =
                PromptCapabilityIdentity::resolve(provider.as_ref(), model, Some(ReasoningEffortLevel::High), 1);
            assert_eq!(identity.context_window, provider.effective_context_size(model));
            assert_eq!(identity.parallel_tools, provider.supports_parallel_tool_config(model));
            assert_eq!(identity.reasoning_effort, !provider.supported_reasoning_efforts(model).is_empty());
            let repeat =
                PromptCapabilityIdentity::resolve(provider.as_ref(), model, Some(ReasoningEffortLevel::High), 1);
            assert_eq!(identity.prefix_hash(7), repeat.prefix_hash(7));
            let refreshed =
                PromptCapabilityIdentity::resolve(provider.as_ref(), model, Some(ReasoningEffortLevel::High), 2);
            assert_ne!(identity.prefix_hash(7), refreshed.prefix_hash(7));
        }
    }

    #[test]
    fn capability_prefix_tracks_changes_for_three_provider_families() {
        for (provider, model, context_window) in [
            ("openai", "openai-fixture", 1_000_000),
            ("anthropic", "claude-fixture", 200_000),
            ("gemini", "gemini-fixture", 32_000),
        ] {
            let identity = PromptCapabilityIdentity {
                provider: provider.into(),
                model: model.into(),
                context_window,
                reasoning_tag: "high".into(),
                reasoning_effort: true,
                parallel_tools: true,
                tools: true,
                caching: true,
                catalog_epoch: 1,
            };
            let baseline = identity.prefix_hash(7);
            assert_eq!(baseline, identity.clone().prefix_hash(7));
            let mut changed = identity.clone();
            changed.catalog_epoch += 1;
            assert_ne!(baseline, changed.prefix_hash(7));
            changed = identity.clone();
            changed.reasoning_tag = "low".into();
            assert_ne!(baseline, changed.prefix_hash(7));
            changed = identity.clone();
            changed.context_window /= 2;
            assert_ne!(baseline, changed.prefix_hash(7));
            changed = identity;
            changed.parallel_tools = false;
            assert_ne!(baseline, changed.prefix_hash(7));
        }
    }
}

/// Stable FNV-1a 64-bit hasher with fixed offset basis.
///
/// `std::collections::hash_map::DefaultHasher` uses per-process random keys,
/// so hashes differ across restarts. Cache and harness keys must be comparable
/// across processes, hence this deterministic alternative shared by
/// [`hash_value`], [`hash_json_value`], and execution-cache builders.
#[derive(Debug, Clone)]
pub struct StableHasher {
    hash: u64,
}

impl Default for StableHasher {
    fn default() -> Self {
        Self { hash: 0xcbf29ce484222325 }
    }
}

impl StableHasher {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Hasher for StableHasher {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.hash ^= u64::from(*byte);
            self.hash = self.hash.wrapping_mul(0x100000001b3);
        }
    }

    fn finish(&self) -> u64 {
        self.hash
    }
}

/// Hash a value with a stable cross-process hash.
pub fn hash_value<T: Hash>(value: &T) -> u64 {
    let mut hasher = StableHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// Hash a serializable value as JSON with a stable cross-process hash.
pub fn hash_json_value<T: Serialize + ?Sized>(value: &T) -> Option<u64> {
    let mut hasher = StableHasher::new();
    serde_json::to_writer(HasherWriter::new(&mut hasher), value).ok().map(|_| {
        hasher.write_u8(0xff);
        hasher.finish()
    })
}

/// Hash tool definitions for cache key computation.
pub fn hash_tool_definitions(tools: Option<&[ToolDefinition]>) -> Option<u64> {
    tools.and_then(hash_json_value)
}

/// Compute the stable instruction digest from explicitly classified sections.
///
/// Unlike earliest-header truncation, this retains stable sections that occur
/// after a dynamic appendix and excludes each runtime section independently.
/// Keep the dynamic list aligned with prompt assembly and provider cache
/// breakpoints.
///
/// Prompt-caching discipline (prefix match): static content first, dynamic
/// last. Planning/full-auto notices are runtime mode transitions and must
/// never perturb the stable prefix hash; they belong in the uncached suffix
/// (or, better, in a `<system-reminder>` message appended to the history).
pub fn stable_system_prefix_hash(system_prompt: &str) -> u64 {
    const DYNAMIC_HEADERS: &[&str] = &[
        "## Active Tools",
        "## Environment",
        "## Active Primary Agent Runtime State",
        "# PLANNING WORKFLOW (READ-ONLY)",
        "# FULL-AUTO: Complete task autonomously until done or blocked.",
        "# FULL-AUTO (PLANNING WORKFLOW): Work autonomously within planning workflow constraints.",
        "[Harness Limits]",
        "[Runtime Tool Catalog]",
        "[Deferred Tools]",
        "[Runtime Context]",
        "[History Directives]",
        "[Context]",
        "[Recovery Mode]",
    ];
    let mut stable = String::with_capacity(system_prompt.len());
    let mut dynamic = false;
    for line in system_prompt.lines() {
        let trimmed = line.trim();
        // Only section headers toggle the dynamic flag. Require the space
        // after `#`/`##` so body lines like `#comment` or `#123` inside a
        // dynamic section cannot prematurely end it and leak dynamic content
        // into the stable hash.
        let is_header = trimmed.starts_with("## ")
            || trimmed.starts_with("# ")
            || trimmed == "#"
            || (trimmed.starts_with('[') && trimmed.ends_with(']'));
        if is_header {
            dynamic = DYNAMIC_HEADERS.contains(&trimmed);
        }
        if !dynamic {
            stable.push_str(line);
            stable.push('\n');
        }
    }
    hash_value(&stable.trim_end())
}

/// Generate a deduplication key for low-signal tool attempts.
pub fn low_signal_attempt_key(name: &str, args: &Value) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut input_len = 0usize;
    if serde_json::to_writer(HashingWriter::new(&mut hash, &mut input_len), args).is_err() {
        for byte in b"{}" {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
            input_len = input_len.saturating_add(1);
        }
    }

    format!("{name}:len{input_len}-fnv{hash:016x}")
}

struct HashingWriter<'a> {
    hash: &'a mut u64,
    input_len: &'a mut usize,
}

impl<'a> HashingWriter<'a> {
    fn new(hash: &'a mut u64, input_len: &'a mut usize) -> Self {
        Self { hash, input_len }
    }
}

impl std::io::Write for HashingWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for byte in buf {
            *self.hash ^= u64::from(*byte);
            *self.hash = self.hash.wrapping_mul(0x100000001b3);
            *self.input_len = self.input_len.saturating_add(1);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct HasherWriter<'a, H> {
    hasher: &'a mut H,
}

impl<'a, H> HasherWriter<'a, H> {
    fn new(hasher: &'a mut H) -> Self {
        Self { hasher }
    }
}

impl<H: Hasher> std::io::Write for HasherWriter<'_, H> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.hasher.write(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
