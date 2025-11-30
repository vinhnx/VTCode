pub mod ranking;
pub mod suggestions;

pub use ranking::SuggestionRanker;
pub use suggestions::CompletionSuggestion;

use crate::code::code_completion::context::CompletionContext;
use crate::code::code_completion::learning::CompletionLearningData;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Type of completion suggestion
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CompletionKind {
    Function,
    Method,
    Variable,
    Class,
    Struct,
    Enum,
    Trait,
    Module,
    Keyword,
    Snippet,
    Import,
    Type,
}

/// Code completion engine
pub struct CompletionEngine {
    suggestion_cache: Arc<RwLock<HashMap<String, Arc<Vec<CompletionSuggestion>>>>>,
    learning_data: Arc<RwLock<CompletionLearningData>>,
    performance_stats: Arc<RwLock<CompletionStats>>,
}

/// Performance statistics for completion engine
#[derive(Debug, Clone, Default)]
pub struct CompletionStats {
    pub total_requests: usize,
    pub cache_hits: usize,
    pub average_response_time_ms: f64,
    pub acceptance_rate: f64,
}

impl CompletionEngine {
    pub fn new() -> Self {
        Self {
            suggestion_cache: Arc::new(RwLock::new(HashMap::new())),
            learning_data: Arc::new(RwLock::new(CompletionLearningData::default())),
            performance_stats: Arc::new(RwLock::new(CompletionStats::default())),
        }
    }

    /// Generate completion suggestions for the given context
    pub async fn complete(&self, context: &CompletionContext) -> Vec<CompletionSuggestion> {
        let arc = self.complete_shared(context).await;
        (*arc).clone()
    }

    /// Return a shared `Arc<Vec<CompletionSuggestion>>` to avoid cloning large collections
    /// when the caller can accept shared ownership.
    pub async fn complete_shared(
        &self,
        context: &CompletionContext,
    ) -> Arc<Vec<CompletionSuggestion>> {
        let cache_key = format!("{}:{}:{}", context.language, context.line, context.column);

        {
            let cache = self.suggestion_cache.read().await;
            if let Some(cached) = cache.get(&cache_key) {
                return Arc::clone(cached);
            }
        }

        // Generate new suggestions
        let mut suggestions = Vec::new();
        let keywords = self.get_language_keywords(&context.language);
        for keyword in keywords {
            if keyword.starts_with(&context.prefix) {
                suggestions.push(CompletionSuggestion {
                    text: keyword.to_string(),
                    kind: CompletionKind::Keyword,
                    confidence: 0.8,
                    context: context.clone(),
                    metadata: HashMap::new(),
                    acceptance_rate: 0.0,
                    learning_data: CompletionLearningData::default(),
                    accepted_count: 0,
                    rejected_count: 0,
                });
            }
        }

        let snippets = self.get_language_snippets(&context.language);
        for snippet in snippets {
            if snippet.label.starts_with(&context.prefix) {
                suggestions.push(CompletionSuggestion {
                    text: snippet.template,
                    kind: CompletionKind::Snippet,
                    confidence: 0.7,
                    context: context.clone(),
                    metadata: HashMap::from([
                        ("label".to_owned(), snippet.label),
                        ("description".to_owned(), snippet.description),
                    ]),
                    acceptance_rate: 0.0,
                    learning_data: CompletionLearningData::default(),
                    accepted_count: 0,
                    rejected_count: 0,
                });
            }
        }

        // Insert into cache as Arc
        let arc = Arc::new(suggestions);
        {
            let mut cache = self.suggestion_cache.write().await;
            cache.insert(cache_key, Arc::clone(&arc));
        }

        arc
    }

    /// Record user feedback on a suggestion
    pub async fn record_feedback(&self, suggestion_id: &str, accepted: bool) {
        let mut learning_data = self.learning_data.write().await;

        // Update acceptance statistics
        let current_rate = learning_data
            .pattern_acceptance
            .get(suggestion_id)
            .copied()
            .unwrap_or(0.0);
        let new_rate = if accepted {
            (current_rate + 1.0) / 2.0
        } else {
            current_rate * 0.9
        };
        learning_data
            .pattern_acceptance
            .insert(suggestion_id.to_string(), new_rate);

        // Update performance stats
        let mut stats = self.performance_stats.write().await;
        stats.total_requests += 1;
        if accepted {
            stats.acceptance_rate = (stats.acceptance_rate * (stats.total_requests - 1) as f64
                + 1.0)
                / stats.total_requests as f64;
        }
    }

    /// Get language-specific keywords
    fn get_language_keywords(&self, language: &str) -> Vec<&'static str> {
        match language {
            "rust" => vec![
                "fn", "let", "mut", "const", "static", "struct", "enum", "impl", "trait", "mod",
                "use", "pub", "crate", "super", "self", "Self", "async", "await", "move", "if",
                "else", "match", "loop", "while", "for", "in", "break", "continue", "return", "as",
                "dyn", "where", "unsafe",
            ],
            "python" => vec![
                "def", "class", "if", "elif", "else", "for", "while", "try", "except", "finally",
                "with", "as", "import", "from", "return", "yield", "lambda", "and", "or", "not",
                "in", "is", "None", "True", "False", "self", "super",
            ],
            "javascript" => vec![
                "function",
                "const",
                "let",
                "var",
                "if",
                "else",
                "for",
                "while",
                "try",
                "catch",
                "finally",
                "return",
                "async",
                "await",
                "class",
                "extends",
                "import",
                "export",
                "from",
                "as",
                "this",
                "super",
                "new",
                "typeof",
                "instanceof",
            ],
            _ => vec![],
        }
    }

    /// Get language-specific snippets
    fn get_language_snippets(&self, language: &str) -> Vec<CodeSnippet> {
        match language {
            "rust" => vec![
                CodeSnippet {
                    label: "fn".to_string(),
                    template: "fn ${1:name}(${2:params}) -> ${3:ReturnType} {\n\t${0:// body}\n}".to_string(),
                    description: "Function declaration".to_string(),
                },
                CodeSnippet {
                    label: "impl".to_string(),
                    template: "impl ${1:Trait} for ${2:Type} {\n\t${0:// implementation}\n}".to_string(),
                    description: "Implementation block".to_string(),
                },
            ],
            "python" => vec![
                CodeSnippet {
                    label: "def".to_string(),
                    template: "def ${1:name}(${2:params}):\n\t${0:# body}".to_string(),
                    description: "Function definition".to_string(),
                },
                CodeSnippet {
                    label: "class".to_string(),
                    template: "class ${1:Name}:\n\tdef __init__(self${2:params}):\n\t\t${0:# initialization}".to_string(),
                    description: "Class definition".to_string(),
                },
            ],
            "javascript" => vec![
                CodeSnippet {
                    label: "func".to_string(),
                    template: "function ${1:name}(${2:params}) {\n\t${0:// body}\n}".to_string(),
                    description: "Function declaration".to_string(),
                },
                CodeSnippet {
                    label: "class".to_string(),
                    template: "class ${1:Name} {\n\tconstructor(${2:params}) {\n\t\t${0:// initialization}\n\t}\n}".to_string(),
                    description: "Class declaration".to_string(),
                },
            ],
            _ => vec![],
        }
    }
}

impl Default for CompletionEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Code snippet template
#[derive(Debug, Clone)]
pub struct CodeSnippet {
    pub label: String,
    pub template: String,
    pub description: String,
}
