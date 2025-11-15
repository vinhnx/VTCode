use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use vtcode_core::config::constants::context as context_defaults;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::tree_sitter::{
    LanguageSupport, SymbolInfo, SymbolKind, TreeSitterAnalyzer,
};

/// Semantic scoring weights for different symbol kinds
/// Higher weights indicate more valuable content that should be preserved during trimming
const SYMBOL_WEIGHT_FUNCTION_METHOD: u32 = 6;
const SYMBOL_WEIGHT_CLASS_STRUCT: u32 = 8;
const SYMBOL_WEIGHT_MODULE_TYPE: u32 = 4;
const SYMBOL_WEIGHT_VARIABLE_CONST: u32 = 2;
const SYMBOL_WEIGHT_IMPORT: u32 = 1;

/// Minimum score threshold for high-value content (functions, classes, etc.)
const HIGH_VALUE_SCORE_THRESHOLD: u8 = SYMBOL_WEIGHT_FUNCTION_METHOD as u8;

/// Configuration for context trimming operations
#[derive(Clone, Copy)]
pub(crate) struct ContextTrimConfig {
    /// Maximum number of tokens to keep in the context
    pub(crate) max_tokens: usize,
    /// Target percentage to trim to when aggressive trimming is needed
    #[allow(dead_code)]
    pub(crate) trim_to_percent: u8,
    /// Number of recent conversation turns to always preserve
    #[allow(dead_code)]
    pub(crate) preserve_recent_turns: usize,
    /// Whether to use semantic analysis for intelligent trimming
    pub(crate) semantic_compression: bool,
    /// Whether to prioritize tool-related messages during trimming
    #[allow(dead_code)]
    pub(crate) tool_aware_retention: bool,
    /// Maximum depth of nested structures to consider during semantic analysis
    #[allow(dead_code)]
    pub(crate) max_structural_depth: usize,
    /// Number of recent tool messages to preserve
    #[allow(dead_code)]
    pub(crate) preserve_recent_tools: usize,
}

impl ContextTrimConfig {
    #[allow(dead_code)]
    pub(crate) fn target_tokens(&self) -> usize {
        let percent = (self.trim_to_percent as u128).clamp(
            context_defaults::MIN_TRIM_RATIO_PERCENT as u128,
            context_defaults::MAX_TRIM_RATIO_PERCENT as u128,
        );
        ((self.max_tokens as u128) * percent / 100) as usize
    }
}

/// Result of a context trimming operation
#[derive(Default)]
#[allow(dead_code)]
pub(crate) struct ContextTrimOutcome {
    /// Number of messages that were removed during trimming
    pub(crate) removed_messages: usize,
}

impl ContextTrimOutcome {
    #[allow(dead_code)]
    pub(crate) fn is_trimmed(&self) -> bool {
        self.removed_messages > 0
    }
}

/// Removes tool-related messages from history while preserving recent and important ones
///
/// This function intelligently prunes tool responses and calls to manage context size,
/// prioritizing recent messages and important tool interactions.
#[allow(dead_code)]
pub(crate) fn prune_unified_tool_responses(
    history: &mut Vec<uni::Message>,
    config: &ContextTrimConfig,
) -> usize {
    if history.is_empty() {
        return 0;
    }

    let keep_from = history.len().saturating_sub(config.preserve_recent_turns);
    if keep_from == 0 {
        return 0;
    }

    // Identify tool messages that should be preserved regardless of age
    let tool_retention_indices = if config.tool_aware_retention && config.preserve_recent_tools > 0
    {
        collect_tool_retention_indices(history, config.preserve_recent_tools)
    } else {
        None
    };

    remove_messages_with_retention(history, keep_from, tool_retention_indices)
}

/// Collects indices of tool messages that should be preserved
fn collect_tool_retention_indices(
    history: &[uni::Message],
    preserve_count: usize,
) -> Option<HashSet<usize>> {
    let mut retained = HashSet::with_capacity(preserve_count);

    // First pass: prioritize recent tool responses
    for (index, message) in history.iter().enumerate().rev() {
        if message.is_tool_response() {
            retained.insert(index);
            if retained.len() >= preserve_count {
                break;
            }
        }
    }

    // Second pass: add tool calls if we still need more
    if retained.len() < preserve_count {
        for (index, message) in history.iter().enumerate().rev() {
            if message.has_tool_calls() {
                retained.insert(index);
                if retained.len() >= preserve_count {
                    break;
                }
            }
        }
    }

    if retained.is_empty() {
        None
    } else {
        Some(retained)
    }
}

/// Removes messages from history while respecting retention indices
fn remove_messages_with_retention(
    history: &mut Vec<uni::Message>,
    keep_from: usize,
    tool_retention_indices: Option<HashSet<usize>>,
) -> usize {
    let mut removed = 0;
    let mut index = 0;

    history.retain(|message| {
        let keep = index >= keep_from
            || (!message.is_tool_response() && !message.has_tool_calls())
            || tool_retention_indices
                .as_ref()
                .map_or(false, |indices| indices.contains(&index));

        if !keep {
            removed += 1;
        }
        index += 1;
        keep
    });

    removed
}

/// Aggressively trims history by removing older messages while preserving recent ones
///
/// This is a fast-path trimming method that simply removes the oldest messages
/// to meet the target size, without semantic analysis.
#[allow(dead_code)]
pub(crate) fn apply_aggressive_trim_unified(
    history: &mut Vec<uni::Message>,
    config: ContextTrimConfig,
) -> usize {
    if history.is_empty() {
        return 0;
    }

    let keep_turns = config
        .preserve_recent_turns
        .clamp(
            context_defaults::MIN_PRESERVE_RECENT_TURNS,
            context_defaults::AGGRESSIVE_PRESERVE_RECENT_TURNS,
        )
        .min(history.len());

    let remove = history.len().saturating_sub(keep_turns);
    if remove == 0 {
        return 0;
    }

    history.drain(0..remove);
    remove
}

/// Enforces context window limits using intelligent semantic trimming
///
/// This function analyzes the semantic importance of messages and removes the least
/// valuable ones first, while preserving recent messages and high-value content.
#[allow(dead_code)]
pub(crate) fn enforce_unified_context_window(
    history: &mut Vec<uni::Message>,
    config: ContextTrimConfig,
    analyzer: Option<&mut TreeSitterAnalyzer>,
    cache: Option<&mut HashMap<u64, u8>>,
) -> ContextTrimOutcome {
    if history.is_empty() || history.len() == 1 {
        return ContextTrimOutcome::default();
    }

    let tokens_per_message: Vec<usize> = history
        .iter()
        .map(approximate_unified_message_tokens)
        .collect();

    let mut total_tokens: usize = tokens_per_message.iter().sum();
    if total_tokens <= config.max_tokens {
        return ContextTrimOutcome::default();
    }

    let semantic_scores = compute_semantic_scores(history, analyzer, cache, &config);
    let preserve_boundary = history.len().saturating_sub(config.preserve_recent_turns);

    let mut removal_set = HashSet::new();

    // Remove low-value messages from early history first
    remove_low_value_messages(
        &mut removal_set,
        &mut total_tokens,
        0..preserve_boundary,
        &semantic_scores,
        &tokens_per_message,
        config.max_tokens,
    );

    // If still over limit, consider messages in the extended range
    if total_tokens > config.max_tokens && preserve_boundary < history.len() - 1 {
        remove_low_value_messages(
            &mut removal_set,
            &mut total_tokens,
            preserve_boundary..history.len() - 1,
            &semantic_scores,
            &tokens_per_message,
            config.max_tokens,
        );
    }

    if removal_set.is_empty() {
        return ContextTrimOutcome::default();
    }

    let removed_messages = apply_removal_set(history, &removal_set);
    ContextTrimOutcome { removed_messages }
}

/// Removes low-value messages from the specified range
fn remove_low_value_messages(
    removal_set: &mut HashSet<usize>,
    total_tokens: &mut usize,
    range: std::ops::Range<usize>,
    semantic_scores: &[u8],
    tokens_per_message: &[usize],
    max_tokens: usize,
) {
    let mut candidates: Vec<usize> = range.collect();
    candidates.sort_by_key(|&i| (semantic_scores[i], i));

    for &index in &candidates {
        if *total_tokens <= max_tokens {
            break;
        }
        // Skip high-value content (functions, classes) - preserve if possible
        if semantic_scores[index] < HIGH_VALUE_SCORE_THRESHOLD {
            removal_set.insert(index);
            *total_tokens = total_tokens.saturating_sub(tokens_per_message[index]);
        }
    }
}

/// Applies the removal set to history and returns count of removed messages
fn apply_removal_set(history: &mut Vec<uni::Message>, removal_set: &HashSet<usize>) -> usize {
    let mut removed_messages = 0;
    let mut current_index = 0;

    history.retain(|_| {
        let keep = !removal_set.contains(&current_index);
        if !keep {
            removed_messages += 1;
        }
        current_index += 1;
        keep
    });

    removed_messages
}

pub(crate) fn load_context_trim_config(vt_cfg: Option<&VTCodeConfig>) -> ContextTrimConfig {
    let context_cfg = vt_cfg.map(|cfg| &cfg.context);
    let max_tokens = std::env::var("VTCODE_CONTEXT_TOKEN_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .or_else(|| {
            context_cfg
                .map(|cfg| cfg.max_context_tokens)
                .filter(|value| *value > 0)
        })
        .unwrap_or(context_defaults::DEFAULT_MAX_TOKENS);

    let trim_to_percent = context_cfg
        .map(|cfg| cfg.trim_to_percent)
        .unwrap_or(context_defaults::DEFAULT_TRIM_TO_PERCENT)
        .clamp(
            context_defaults::MIN_TRIM_RATIO_PERCENT,
            context_defaults::MAX_TRIM_RATIO_PERCENT,
        );

    let preserve_recent_turns = context_cfg
        .map(|cfg| cfg.preserve_recent_turns)
        .unwrap_or(context_defaults::DEFAULT_PRESERVE_RECENT_TURNS)
        .max(context_defaults::MIN_PRESERVE_RECENT_TURNS);

    ContextTrimConfig {
        max_tokens,
        trim_to_percent,
        preserve_recent_turns,
        semantic_compression: context_cfg
            .map(|cfg| cfg.semantic_compression)
            .unwrap_or(context_defaults::DEFAULT_SEMANTIC_COMPRESSION_ENABLED),
        tool_aware_retention: context_cfg
            .map(|cfg| cfg.tool_aware_retention)
            .unwrap_or(context_defaults::DEFAULT_TOOL_AWARE_RETENTION_ENABLED),
        max_structural_depth: context_cfg
            .map(|cfg| cfg.max_structural_depth)
            .unwrap_or(context_defaults::DEFAULT_MAX_STRUCTURAL_DEPTH),
        preserve_recent_tools: context_cfg
            .map(|cfg| cfg.preserve_recent_tools)
            .unwrap_or(context_defaults::DEFAULT_PRESERVE_RECENT_TOOLS),
    }
}

/// Approximates the token count for a message using character-based estimation
///
/// This provides a fast approximation without requiring actual tokenization.
#[allow(dead_code)]
fn approximate_unified_message_tokens(message: &uni::Message) -> usize {
    let mut total_chars = message.content.as_text().len();
    total_chars += message.role.as_generic_str().len();

    if let Some(tool_calls) = &message.tool_calls {
        total_chars += tool_calls.iter().fold(0, |acc, call| {
            acc + call.id.len()
                + call.call_type.len()
                + call
                    .function
                    .as_ref()
                    .expect("Tool call must have function")
                    .name
                    .len()
                + call
                    .function
                    .as_ref()
                    .expect("Tool call must have function")
                    .arguments
                    .len()
        });
    }

    if let Some(tool_call_id) = &message.tool_call_id {
        total_chars += tool_call_id.len();
    }

    total_chars.div_ceil(context_defaults::CHAR_PER_TOKEN_APPROX)
}

/// Represents a code block extracted from message content
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct CodeBlock {
    /// Optional language hint (e.g., "rust", "python")
    language_hint: Option<String>,
    /// The actual code content
    content: String,
}

/// Computes semantic importance scores for all messages in history
///
/// Messages with higher scores contain more valuable content (functions, classes, etc.)
/// and should be preserved during trimming operations.
#[allow(dead_code)]
fn compute_semantic_scores(
    history: &[uni::Message],
    analyzer: Option<&mut TreeSitterAnalyzer>,
    cache: Option<&mut HashMap<u64, u8>>,
    config: &ContextTrimConfig,
) -> Vec<u8> {
    if !config.semantic_compression || history.is_empty() {
        return vec![0; history.len()];
    }

    let Some(analyzer) = analyzer else {
        return vec![0; history.len()];
    };

    if let Some(cache_map) = cache {
        compute_scores_with_cache(history, analyzer, config, cache_map)
    } else {
        history
            .iter()
            .map(|message| compute_semantic_score(message, analyzer, config))
            .collect()
    }
}

/// Computes scores using a cache to avoid redundant analysis
fn compute_scores_with_cache(
    history: &[uni::Message],
    analyzer: &mut TreeSitterAnalyzer,
    config: &ContextTrimConfig,
    cache_map: &mut HashMap<u64, u8>,
) -> Vec<u8> {
    let mut scores = Vec::with_capacity(history.len());
    for message in history {
        let cache_key = message_semantic_hash(message);
        let score = if let Some(&cached) = cache_map.get(&cache_key) {
            cached
        } else {
            let computed = compute_semantic_score(message, analyzer, config);
            cache_map.insert(cache_key, computed);
            computed
        };
        scores.push(score);
    }
    scores
}

/// Computes semantic importance score for a single message
///
/// The score is based on:
/// - Code structure (functions, classes, modules)
/// - Tool-related content (responses, calls)
/// - Origin tool tracking
#[allow(dead_code)]
fn compute_semantic_score(
    message: &uni::Message,
    analyzer: &mut TreeSitterAnalyzer,
    config: &ContextTrimConfig,
) -> u8 {
    let message_text = message.content.as_text();
    let mut code_blocks = extract_code_blocks(&message_text);

    // If no explicit code blocks but content looks like code, treat entire message as code
    if code_blocks.is_empty()
        && analyzer
            .detect_language_from_content(&message_text)
            .is_some()
    {
        code_blocks.push(CodeBlock {
            language_hint: None,
            content: message_text.clone(),
        });
    }

    if code_blocks.is_empty() {
        return 0;
    }

    let total_score: u32 = code_blocks
        .iter()
        .filter_map(|block| analyze_code_block(block, analyzer, config))
        .fold(0, |acc, score| acc.saturating_add(score));

    if total_score == 0 {
        return 0;
    }

    let tool_bonus = calculate_tool_bonus(message, config);
    total_score.saturating_add(tool_bonus).min(u8::MAX as u32) as u8
}

/// Analyzes a single code block and returns its semantic score
fn analyze_code_block(
    block: &CodeBlock,
    analyzer: &mut TreeSitterAnalyzer,
    config: &ContextTrimConfig,
) -> Option<u32> {
    let snippet = block.content.trim();
    if snippet.is_empty() {
        return None;
    }

    let language = block
        .language_hint
        .as_deref()
        .and_then(language_hint_to_support)
        .or_else(|| analyzer.detect_language_from_content(snippet))?;

    let tree = analyzer.parse(snippet, language).ok()?;
    let symbols = analyzer.extract_symbols(&tree, snippet, language).ok()?;

    let block_score = score_symbols(&symbols, config);
    Some(if block_score > 0 { block_score } else { 1 })
}

/// Calculates bonus points for tool-related content
fn calculate_tool_bonus(message: &uni::Message, config: &ContextTrimConfig) -> u32 {
    let base_bonus: u32 = if message.is_tool_response() || message.has_tool_calls() {
        2
    } else {
        0
    };

    let origin_bonus: u32 = if message.origin_tool.is_some() && config.tool_aware_retention {
        1
    } else {
        0
    };

    base_bonus.saturating_add(origin_bonus)
}

/// Scores symbols based on their type and importance
///
/// Different symbol types have different weights for context preservation.
#[allow(dead_code)]
fn score_symbols(symbols: &[SymbolInfo], config: &ContextTrimConfig) -> u32 {
    symbols
        .iter()
        .filter(|symbol| {
            estimate_scope_depth(symbol.scope.as_deref()) <= config.max_structural_depth
        })
        .map(|symbol| symbol_weight(&symbol.kind))
        .fold(0, |acc, weight| acc.saturating_add(weight))
}

/// Returns the weight for a given symbol kind
fn symbol_weight(kind: &SymbolKind) -> u32 {
    match kind {
        SymbolKind::Function | SymbolKind::Method => SYMBOL_WEIGHT_FUNCTION_METHOD,
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::Interface | SymbolKind::Trait => {
            SYMBOL_WEIGHT_CLASS_STRUCT
        }
        SymbolKind::Module | SymbolKind::Type => SYMBOL_WEIGHT_MODULE_TYPE,
        SymbolKind::Variable | SymbolKind::Constant => SYMBOL_WEIGHT_VARIABLE_CONST,
        SymbolKind::Import => SYMBOL_WEIGHT_IMPORT,
    }
}

/// Estimates the nesting depth of a scope string
///
/// Deeper scopes are considered less important for context preservation.
#[allow(dead_code)]
fn estimate_scope_depth(scope: Option<&str>) -> usize {
    scope.map_or(0, |raw| {
        raw.split(|ch: char| matches!(ch, ':' | '.' | '#'))
            .filter(|segment| !segment.is_empty())
            .count()
    })
}

/// Extracts code blocks from markdown-formatted text
///
/// Handles both fenced code blocks with language hints and plain code blocks.
#[allow(dead_code)]
fn extract_code_blocks(source: &str) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut current_language: Option<String> = None;
    let mut current_content = String::new();

    for line in source.lines() {
        let trimmed = line.trim_start();
        if let Some(hint) = trimmed.strip_prefix("```") {
            if in_block {
                // End of code block
                if !current_content.is_empty() {
                    blocks.push(CodeBlock {
                        language_hint: current_language.take(),
                        content: current_content.trim_end().to_string(),
                    });
                }
                current_content.clear();
                in_block = false;
            } else {
                // Start of code block
                in_block = true;
                current_language = Some(hint.trim().to_string()).filter(|s| !s.is_empty());
                current_content.clear();
            }
            continue;
        }

        if in_block {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Handle unclosed code block at end of text
    if in_block && !current_content.is_empty() {
        blocks.push(CodeBlock {
            language_hint: current_language.take(),
            content: current_content.trim_end().to_string(),
        });
    }

    blocks
}

/// Converts language hint string to LanguageSupport enum
///
/// Supports common language names and file extensions.
#[allow(dead_code)]
fn language_hint_to_support(hint: &str) -> Option<LanguageSupport> {
    match hint
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .as_str()
    {
        "rust" | "rs" => Some(LanguageSupport::Rust),
        "python" | "py" => Some(LanguageSupport::Python),
        "javascript" | "js" => Some(LanguageSupport::JavaScript),
        "typescript" | "ts" | "tsx" => Some(LanguageSupport::TypeScript),
        "go" | "golang" => Some(LanguageSupport::Go),
        "java" => Some(LanguageSupport::Java),
        "bash" | "sh" | "shell" => Some(LanguageSupport::Bash),
        "swift" => Some(LanguageSupport::Swift),
        _ => None,
    }
}

/// Computes a hash for message semantic content caching
///
/// Messages with identical semantic content will have the same hash,
/// allowing us to cache expensive semantic analysis results.
#[allow(dead_code)]
fn message_semantic_hash(message: &uni::Message) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    let mut hasher = DefaultHasher::new();
    message.role.as_generic_str().hash(&mut hasher);
    message.content.as_text().hash(&mut hasher);

    if let Some(tool_call_id) = &message.tool_call_id {
        tool_call_id.hash(&mut hasher);
    }

    if let Some(tool_calls) = &message.tool_calls {
        for call in tool_calls {
            call.id.hash(&mut hasher);
            call.function
                .as_ref()
                .expect("Tool call must have function")
                .name
                .hash(&mut hasher);
            call.function
                .as_ref()
                .expect("Tool call must have function")
                .arguments
                .hash(&mut hasher);
        }
    }

    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::config::constants::context as context_defaults;
    use vtcode_core::tools::tree_sitter::TreeSitterAnalyzer;

    #[test]
    fn test_enforce_unified_context_window_trims_and_preserves_latest() {
        let mut history: Vec<uni::Message> = (0..12)
            .map(|i| uni::Message::assistant(format!("assistant step {}", i)))
            .collect();
        let original_len = history.len();
        let config = ContextTrimConfig {
            max_tokens: 18,
            trim_to_percent: 70,
            preserve_recent_turns: 3,
            semantic_compression: false,
            tool_aware_retention: false,
            max_structural_depth: context_defaults::DEFAULT_MAX_STRUCTURAL_DEPTH,
            preserve_recent_tools: context_defaults::DEFAULT_PRESERVE_RECENT_TOOLS,
        };

        let outcome = enforce_unified_context_window(&mut history, config, None, None);

        assert!(outcome.is_trimmed());
        assert_eq!(original_len - history.len(), outcome.removed_messages);

        let remaining_tokens: usize = history.iter().map(approximate_unified_message_tokens).sum();
        assert!(remaining_tokens <= config.max_tokens);

        let last_content = history
            .last()
            .map(|msg| msg.content.as_text())
            .unwrap_or_default();
        assert!(last_content.contains("assistant step 11"));
    }

    #[test]
    fn test_prune_unified_tool_responses_respects_recent_history() {
        let mut history: Vec<uni::Message> = vec![
            uni::Message::user("keep".to_string()),
            uni::Message::tool_response("call_1".to_string(), "{\"result\":1}".to_string()),
            uni::Message::assistant("assistant0".to_string()),
            uni::Message::user("keep2".to_string()),
            {
                let mut msg = uni::Message::assistant("assistant_with_tool".to_string());
                msg.tool_calls = Some(vec![uni::ToolCall::function(
                    "call_2".to_string(),
                    "tool_b".to_string(),
                    "{}".to_string(),
                )]);
                msg
            },
            uni::Message::tool_response("call_2".to_string(), "{\"result\":2}".to_string()),
        ];

        let config = ContextTrimConfig {
            max_tokens: 140,
            trim_to_percent: 80,
            preserve_recent_turns: 4,
            semantic_compression: false,
            tool_aware_retention: false,
            max_structural_depth: context_defaults::DEFAULT_MAX_STRUCTURAL_DEPTH,
            preserve_recent_tools: context_defaults::DEFAULT_PRESERVE_RECENT_TOOLS,
        };

        let removed = prune_unified_tool_responses(&mut history, &config);

        assert_eq!(removed, 1);
        assert!(history.len() >= 4);
        assert_eq!(
            history.first().unwrap().content.as_text(),
            "keep".to_string()
        );
        assert!(history.iter().any(|msg| msg.is_tool_response()));
    }

    #[test]
    fn test_apply_aggressive_trim_unified_limits_history() {
        let mut history: Vec<uni::Message> = (0..15)
            .map(|i| uni::Message::assistant(format!("assistant step {i}")))
            .collect();
        let config = ContextTrimConfig {
            max_tokens: 140,
            trim_to_percent: 80,
            preserve_recent_turns: 10,
            semantic_compression: false,
            tool_aware_retention: false,
            max_structural_depth: context_defaults::DEFAULT_MAX_STRUCTURAL_DEPTH,
            preserve_recent_tools: context_defaults::DEFAULT_PRESERVE_RECENT_TOOLS,
        };

        let removed = apply_aggressive_trim_unified(&mut history, config);

        let expected_len = context_defaults::AGGRESSIVE_PRESERVE_RECENT_TURNS;
        assert_eq!(removed, 15 - expected_len);
        assert_eq!(history.len(), expected_len);
        let expected_first = format!(
            "assistant step {}",
            15 - context_defaults::AGGRESSIVE_PRESERVE_RECENT_TURNS
        );
        assert!(
            history
                .first()
                .map(|msg| msg.content.as_text())
                .unwrap_or_default()
                .contains(&expected_first)
        );
    }

    #[test]
    fn test_prune_unified_tool_responses_preserves_recent_tool_payloads() {
        let mut history: Vec<uni::Message> = vec![
            uni::Message::tool_response("call_0".to_string(), "{\"result\":0}".to_string()),
            uni::Message::assistant("assistant_turn".to_string()),
            uni::Message::tool_response("call_1".to_string(), "{\"result\":1}".to_string()),
            {
                let mut msg = uni::Message::assistant("assistant_with_tool".to_string());
                msg.tool_calls = Some(vec![uni::ToolCall::function(
                    "call_2".to_string(),
                    "tool_b".to_string(),
                    "{}".to_string(),
                )]);
                msg
            },
            uni::Message::tool_response("call_2".to_string(), "{\"result\":2}".to_string()),
        ];

        let config = ContextTrimConfig {
            max_tokens: 140,
            trim_to_percent: 80,
            preserve_recent_turns: 2,
            semantic_compression: false,
            tool_aware_retention: true,
            max_structural_depth: context_defaults::DEFAULT_MAX_STRUCTURAL_DEPTH,
            preserve_recent_tools: 2,
        };

        let removed = prune_unified_tool_responses(&mut history, &config);

        assert_eq!(removed, 1);
        assert_eq!(history.len(), 4);
        let retained_tool_messages: Vec<String> = history
            .iter()
            .filter(|msg| msg.is_tool_response())
            .map(|msg| msg.content.as_text().to_string())
            .collect();
        assert_eq!(retained_tool_messages.len(), 2);
        assert!(
            retained_tool_messages
                .iter()
                .any(|content| content.contains("result\":1"))
        );
        assert!(
            retained_tool_messages
                .iter()
                .any(|content| content.contains("result\":2"))
        );
        assert!(
            history
                .first()
                .map(|msg| msg.content.as_text())
                .unwrap_or_default()
                .contains("assistant_turn")
        );
    }

    #[test]
    fn test_semantic_compression_prioritizes_structural_code() {
        let mut history: Vec<uni::Message> = vec![
            uni::Message::assistant(
                "intro summary context details that should be trimmed".to_string(),
            ),
            uni::Message::assistant(
                "```rust\nfn important_util() {\n    println!(\"hi\");\n}\n```".to_string(),
            ),
            uni::Message::assistant(
                "follow up narrative continues with additional prose".to_string(),
            ),
            uni::Message::assistant("recent note".to_string()),
        ];

        let config = ContextTrimConfig {
            max_tokens: 18,
            trim_to_percent: 60,
            preserve_recent_turns: 1,
            semantic_compression: true,
            tool_aware_retention: false,
            max_structural_depth: context_defaults::DEFAULT_MAX_STRUCTURAL_DEPTH,
            preserve_recent_tools: context_defaults::DEFAULT_PRESERVE_RECENT_TOOLS,
        };

        let mut analyzer =
            TreeSitterAnalyzer::new().expect("Failed to create analyzer for semantic test");

        let outcome =
            enforce_unified_context_window(&mut history, config, Some(&mut analyzer), None);

        assert!(outcome.is_trimmed());
        assert!(
            history
                .iter()
                .any(|msg| msg.content.as_text().contains("fn important_util"))
        );
        assert!(history.len() <= 3);
        assert!(
            !history
                .iter()
                .any(|msg| msg.content.as_text().contains("intro summary"))
        );
    }

    #[test]
    fn test_origin_tool_weighting_in_semantic_scoring() {
        // Test that messages with origin_tool get higher semantic scores
        let mut history: Vec<uni::Message> = vec![
            uni::Message::user("initial question".to_string()),
            {
                let mut msg = uni::Message::assistant("using grep to search".to_string());
                msg.tool_calls = Some(vec![uni::ToolCall::function(
                    "call_1".to_string(),
                    "grep_search".to_string(),
                    "{}".to_string(),
                )]);
                msg
            },
            {
                let mut msg = uni::Message::tool_response(
                    "call_1".to_string(),
                    "{\"matches\": [\"foo.rs\"]}".to_string(),
                );
                msg.origin_tool = Some("grep_search".to_string());
                msg
            },
            uni::Message::assistant("Found matches in grep".to_string()),
            {
                let mut msg =
                    uni::Message::assistant("using read_file on the grep result".to_string());
                msg.tool_calls = Some(vec![uni::ToolCall::function(
                    "call_2".to_string(),
                    "read_file".to_string(),
                    "{}".to_string(),
                )]);
                msg
            },
            {
                let mut msg = uni::Message::tool_response(
                    "call_2".to_string(),
                    "{\"content\": \"file content here\"}".to_string(),
                );
                msg.origin_tool = Some("read_file".to_string());
                msg
            },
            uni::Message::assistant("File content processed".to_string()),
        ];

        let config = ContextTrimConfig {
            max_tokens: 60,
            trim_to_percent: 80,
            preserve_recent_turns: 2,
            semantic_compression: true,
            tool_aware_retention: true,
            max_structural_depth: context_defaults::DEFAULT_MAX_STRUCTURAL_DEPTH,
            preserve_recent_tools: 2,
        };

        let mut analyzer = TreeSitterAnalyzer::new().expect("Failed to create analyzer");

        let original_len = history.len();
        let outcome =
            enforce_unified_context_window(&mut history, config, Some(&mut analyzer), None);

        // Verify that we trimmed some messages
        assert!(outcome.is_trimmed());
        assert!(history.len() < original_len);

        // Verify that recent tool responses are preserved
        let has_tool_response = history.iter().any(|msg| msg.is_tool_response());
        assert!(
            has_tool_response,
            "Should preserve at least one tool response"
        );

        // Verify that messages with origin_tool are preserved
        let has_origin_tool = history.iter().any(|msg| msg.origin_tool.is_some());
        assert!(
            has_origin_tool,
            "Should preserve at least one message with origin_tool"
        );
    }
}
