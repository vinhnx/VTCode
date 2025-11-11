use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use vtcode_core::config::constants::context as context_defaults;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::tree_sitter::{
    LanguageSupport, SymbolInfo, SymbolKind, TreeSitterAnalyzer,
};

#[derive(Clone, Copy)]
pub(crate) struct ContextTrimConfig {
    pub(crate) max_tokens: usize,
    #[allow(dead_code)]
    pub(crate) trim_to_percent: u8,
    #[allow(dead_code)]
    pub(crate) preserve_recent_turns: usize,
    pub(crate) semantic_compression: bool,
    #[allow(dead_code)]
    pub(crate) tool_aware_retention: bool,
    #[allow(dead_code)]
    pub(crate) max_structural_depth: usize,
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

#[derive(Default)]
#[allow(dead_code)]
pub(crate) struct ContextTrimOutcome {
    pub(crate) removed_messages: usize,
}

impl ContextTrimOutcome {
    #[allow(dead_code)]
    pub(crate) fn is_trimmed(&self) -> bool {
        self.removed_messages > 0
    }
}

#[allow(dead_code)]
pub(crate) fn prune_unified_tool_responses(
    history: &mut Vec<uni::Message>,
    config: &ContextTrimConfig,
) -> usize {
    if history.is_empty() {
        return 0;
    }

    let keep_from = history.len().saturating_sub(config.preserve_recent_turns);

    let tool_retention_indices = if config.tool_aware_retention && config.preserve_recent_tools > 0
    {
        let mut retained: HashSet<usize> = HashSet::with_capacity(config.preserve_recent_tools);

        for index in (0..history.len()).rev() {
            if history[index].is_tool_response() {
                retained.insert(index);
                if retained.len() >= config.preserve_recent_tools {
                    break;
                }
            }
        }

        if retained.len() < config.preserve_recent_tools {
            for index in (0..history.len()).rev() {
                if history[index].has_tool_calls() {
                    retained.insert(index);
                    if retained.len() >= config.preserve_recent_tools {
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
    } else {
        None
    };

    if keep_from == 0 {
        return 0;
    }

    let mut removed = 0usize;
    let mut index = 0usize;
    history.retain(|message| {
        let contains_tool_payload = message.is_tool_response() || message.has_tool_calls();
        let keep_due_to_recent_turn = index >= keep_from;
        let keep_due_to_tool_retention = tool_retention_indices
            .as_ref()
            .map(|indices| indices.contains(&index))
            .unwrap_or(false);
        let keep = keep_due_to_recent_turn || !contains_tool_payload || keep_due_to_tool_retention;
        if !keep {
            removed += 1;
        }
        index += 1;
        keep
    });
    removed
}

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

#[allow(dead_code)]
pub(crate) fn enforce_unified_context_window(
    history: &mut Vec<uni::Message>,
    config: ContextTrimConfig,
    analyzer: Option<&mut TreeSitterAnalyzer>,
    cache: Option<&mut HashMap<u64, u8>>,
) -> ContextTrimOutcome {
    if history.is_empty() {
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

    let target_tokens = config.target_tokens();
    let semantic_scores = compute_semantic_scores(history, analyzer, cache, &config);
    let last_index = history.len().saturating_sub(1);
    let mut removal_set: HashSet<usize> = HashSet::new();

    if history.len() == 1 {
        return ContextTrimOutcome::default();
    }

    let preserve_boundary = history.len().saturating_sub(config.preserve_recent_turns);
    let mut early_candidates: Vec<usize> = (0..preserve_boundary).collect();
    early_candidates.sort_by(|a, b| {
        semantic_scores[*a]
            .cmp(&semantic_scores[*b])
            .then_with(|| a.cmp(b))
    });

    for index in early_candidates {
        if total_tokens <= config.max_tokens {
            break;
        }
        removal_set.insert(index);
        total_tokens = total_tokens.saturating_sub(tokens_per_message[index]);
        if total_tokens <= target_tokens {
            break;
        }
    }

    if total_tokens > config.max_tokens && last_index > 0 {
        let mut extended_candidates: Vec<usize> = (preserve_boundary..last_index).collect();
        extended_candidates.sort_by(|a, b| {
            semantic_scores[*a]
                .cmp(&semantic_scores[*b])
                .then_with(|| a.cmp(b))
        });

        for index in extended_candidates {
            if total_tokens <= config.max_tokens {
                break;
            }
            removal_set.insert(index);
            total_tokens = total_tokens.saturating_sub(tokens_per_message[index]);
            if total_tokens <= target_tokens {
                break;
            }
        }
    }

    if removal_set.is_empty() {
        return ContextTrimOutcome::default();
    }

    let mut removed_messages = 0usize;
    let mut current_index = 0usize;
    history.retain(|_| {
        let keep = !removal_set.contains(&current_index);
        if !keep {
            removed_messages += 1;
        }
        current_index += 1;
        keep
    });

    ContextTrimOutcome { removed_messages }
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

#[allow(dead_code)]
fn approximate_unified_message_tokens(message: &uni::Message) -> usize {
    let mut total_chars = message.content.as_text().len();
    total_chars += message.role.as_generic_str().len();

    if let Some(tool_calls) = &message.tool_calls {
        for call in tool_calls {
            total_chars += call.id.len();
            total_chars += call.call_type.len();
            total_chars += call.function.name.len();
            total_chars += call.function.arguments.len();
        }
    }

    if let Some(tool_call_id) = &message.tool_call_id {
        total_chars += tool_call_id.len();
    }

    total_chars.div_ceil(context_defaults::CHAR_PER_TOKEN_APPROX)
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct CodeBlock {
    language_hint: Option<String>,
    content: String,
}

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
        let mut scores = Vec::with_capacity(history.len());
        for message in history {
            let cache_key = message_semantic_hash(message);
            let score = if let Some(value) = cache_map.get(&cache_key).copied() {
                value
            } else {
                let computed = compute_semantic_score(message, analyzer, config);
                cache_map.insert(cache_key, computed);
                computed
            };
            scores.push(score);
        }
        scores
    } else {
        history
            .iter()
            .map(|message| compute_semantic_score(message, analyzer, config))
            .collect()
    }
}

#[allow(dead_code)]
fn compute_semantic_score(
    message: &uni::Message,
    analyzer: &mut TreeSitterAnalyzer,
    config: &ContextTrimConfig,
) -> u8 {
    let message_text = message.content.as_text();
    let mut code_blocks = extract_code_blocks(&message_text);

    if code_blocks.is_empty() {
        if analyzer
            .detect_language_from_content(&message_text)
            .is_some()
        {
            code_blocks.push(CodeBlock {
                language_hint: None,
                content: message_text.clone(),
            });
        } else {
            return 0;
        }
    }

    let mut total_score: u32 = 0;

    for block in code_blocks {
        let snippet = block.content.trim();
        if snippet.is_empty() {
            continue;
        }

        let mut language = block
            .language_hint
            .as_deref()
            .and_then(language_hint_to_support);

        if language.is_none() {
            language = analyzer.detect_language_from_content(snippet);
        }

        let Some(language) = language else {
            continue;
        };

        let tree = match analyzer.parse(snippet, language) {
            Ok(tree) => tree,
            Err(_) => continue,
        };

        let symbols = match analyzer.extract_symbols(&tree, snippet, language) {
            Ok(symbols) => symbols,
            Err(_) => Vec::new(),
        };

        let block_score = score_symbols(&symbols, config);
        if block_score > 0 {
            total_score = total_score.saturating_add(block_score);
        } else {
            total_score = total_score.saturating_add(1);
        }
    }

    if total_score == 0 {
        0
    } else {
        // Base bonus for tool responses/calls
        let mut base_bonus: u32 = if message.is_tool_response() || message.has_tool_calls() {
            2
        } else {
            0
        };

        // Additional bonus if this message originated from an actively-used tool
        if message.origin_tool.is_some() && config.tool_aware_retention {
            base_bonus = base_bonus.saturating_add(1);
        }

        let score = total_score.saturating_add(base_bonus).min(u8::MAX as u32);
        score as u8
    }
}

#[allow(dead_code)]
fn score_symbols(symbols: &[SymbolInfo], config: &ContextTrimConfig) -> u32 {
    let mut total = 0u32;

    for symbol in symbols {
        let scope_depth = estimate_scope_depth(symbol.scope.as_deref());
        if scope_depth > config.max_structural_depth {
            continue;
        }

        let weight = match symbol.kind {
            SymbolKind::Function | SymbolKind::Method => 6,
            SymbolKind::Class | SymbolKind::Struct | SymbolKind::Interface | SymbolKind::Trait => 8,
            SymbolKind::Module | SymbolKind::Type => 4,
            SymbolKind::Variable | SymbolKind::Constant => 2,
            SymbolKind::Import => 1,
        };

        total = total.saturating_add(weight);
    }

    total
}

#[allow(dead_code)]
fn estimate_scope_depth(scope: Option<&str>) -> usize {
    scope
        .map(|raw| {
            raw.split(|ch: char| matches!(ch, ':' | '.' | '#'))
                .filter(|segment| !segment.is_empty())
                .count()
        })
        .unwrap_or(0)
}

#[allow(dead_code)]
fn extract_code_blocks(source: &str) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut current_language: Option<String> = None;
    let mut current_content = String::new();

    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if in_block {
                let language_hint = current_language.take();
                if !current_content.is_empty() {
                    blocks.push(CodeBlock {
                        language_hint,
                        content: current_content.trim_end().to_string(),
                    });
                }
                current_content.clear();
                in_block = false;
            } else {
                in_block = true;
                let hint = trimmed.trim_start_matches("```").trim();
                current_language = if hint.is_empty() {
                    None
                } else {
                    Some(hint.to_string())
                };
                current_content.clear();
            }
            continue;
        }

        if in_block {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    if in_block && !current_content.is_empty() {
        blocks.push(CodeBlock {
            language_hint: current_language.take(),
            content: current_content.trim_end().to_string(),
        });
    }

    blocks
}

#[allow(dead_code)]
fn language_hint_to_support(hint: &str) -> Option<LanguageSupport> {
    let normalized = hint.trim().trim_start_matches('.').to_ascii_lowercase();
    match normalized.as_str() {
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

#[allow(dead_code)]
fn message_semantic_hash(message: &uni::Message) -> u64 {
    use std::collections::hash_map::DefaultHasher;

    let mut hasher = DefaultHasher::new();
    message.role.as_generic_str().hash(&mut hasher);
    message.content.as_text().hash(&mut hasher);

    if let Some(tool_call_id) = &message.tool_call_id {
        tool_call_id.hash(&mut hasher);
    }

    if let Some(tool_calls) = &message.tool_calls {
        for call in tool_calls {
            call.id.hash(&mut hasher);
            call.function.name.hash(&mut hasher);
            call.function.arguments.hash(&mut hasher);
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
