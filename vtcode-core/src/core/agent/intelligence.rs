//! Research-preview intelligence layer for enhanced code understanding and context awareness
//!
//! This module implements modern coding agent capabilities including:
//! - Context-aware code understanding
//! - Intelligent code completion
//! - Learning and adaptation systems

use crate::tools::tree_sitter::{CodeAnalysis, TreeSitterAnalyzer};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Location information for symbols and references
#[derive(Debug, Clone, Serialize, Deserialize, Eq, Hash, PartialEq)]
pub struct Location {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

/// Compaction strategy options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompactionStrategy {
    Conservative,
    Balanced,
    Aggressive,
}

/// Intelligent compaction decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionDecision {
    pub should_compact: bool,
    pub strategy: CompactionStrategy,
    pub reasoning: String,
    pub estimated_benefit: usize,
}

/// Enhanced context representation with semantic understanding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticContext {
    /// Current file being worked on
    pub current_file: Option<PathBuf>,
    /// Recently accessed files (LRU cache)
    pub recent_files: Vec<PathBuf>,
    /// Current cursor position and selection
    pub cursor_context: Option<CursorContext>,
    /// Project-wide symbols and definitions
    pub symbol_table: HashMap<String, SymbolInfo>,
    /// Code patterns and conventions observed
    pub code_patterns: Vec<CodePattern>,
    /// Import relationships and dependencies
    pub dependency_graph: HashMap<String, Vec<String>>,
}

/// Cursor position and surrounding context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorContext {
    pub line: usize,
    pub column: usize,
    pub selected_text: Option<String>,
    pub surrounding_lines: Vec<String>,
    pub current_function: Option<String>,
    pub current_class: Option<String>,
}

/// Enhanced symbol information with usage patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub location: Location,
    pub definition: String,
    pub usages: Vec<Location>,
    pub related_symbols: Vec<String>,
    pub confidence_score: f64,
}

/// Types of symbols the agent can understand
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Class,
    Struct,
    Enum,
    Variable,
    Constant,
    Module,
    Type,
    Macro,
}

/// Code patterns and conventions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodePattern {
    pub pattern_type: PatternType,
    pub description: String,
    pub examples: Vec<String>,
    pub frequency: usize,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PatternType {
    NamingConvention,
    ErrorHandling,
    AsyncPattern,
    DesignPattern,
    CodeStyle,
}

/// Intelligent code completion suggestions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionSuggestion {
    pub text: String,
    pub kind: CompletionKind,
    pub relevance_score: f64,
    pub context_info: String,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompletionKind {
    Function,
    Variable,
    Type,
    Keyword,
    Snippet,
    Import,
}

/// Research-preview intelligence engine
pub struct IntelligenceEngine {
    analyzer: TreeSitterAnalyzer,
    context: Arc<RwLock<SemanticContext>>,
    pattern_learner: PatternLearner,
    completion_engine: CompletionEngine,
}

impl IntelligenceEngine {
    /// Create a new intelligence engine
    pub fn new() -> Result<Self> {
        let analyzer = TreeSitterAnalyzer::new()
            .map_err(|e| anyhow!("Failed to initialize tree-sitter analyzer: {}", e))?;

        let context = Arc::new(RwLock::new(SemanticContext {
            current_file: None,
            recent_files: Vec::new(),
            cursor_context: None,
            symbol_table: HashMap::new(),
            code_patterns: Vec::new(),
            dependency_graph: HashMap::new(),
        }));

        Ok(Self {
            analyzer,
            context,
            pattern_learner: PatternLearner::new(),
            completion_engine: CompletionEngine::new(),
        })
    }

    /// Analyze current context and update semantic understanding
    pub async fn analyze_context(&mut self, workspace_root: &Path) -> Result<()> {
        // Analyze workspace structure
        self.analyze_workspace_structure(workspace_root).await?;

        // Build symbol table
        self.build_symbol_table(workspace_root).await?;

        // Learn code patterns
        {
            let context = self.context.read().await;
            self.pattern_learner.learn_patterns(&context).await?;
        }

        // Update dependency graph
        self.update_dependency_graph().await?;

        Ok(())
    }

    /// Get intelligent code completion suggestions
    pub async fn get_completions(
        &mut self,
        file_path: &Path,
        cursor_line: usize,
        cursor_column: usize,
        prefix: &str,
    ) -> Result<Vec<CompletionSuggestion>> {
        let context = self.context.read().await;

        // Get file analysis
        let source_code = tokio::fs::read_to_string(file_path).await?;
        let analysis = self
            .analyzer
            .analyze_file_with_tree_sitter(file_path, &source_code)?;

        // Generate context-aware completions
        self.completion_engine
            .generate_completions(&context, &analysis, cursor_line, cursor_column, prefix)
            .await
    }

    /// Update cursor context for better understanding
    pub async fn update_cursor_context(
        &mut self,
        file_path: &Path,
        line: usize,
        column: usize,
        selected_text: Option<String>,
    ) -> Result<()> {
        let source_code = tokio::fs::read_to_string(file_path).await?;
        let lines: Vec<String> = source_code.lines().map(|s| s.to_string()).collect();

        // Extract surrounding context
        let start_line = line.saturating_sub(5);
        let end_line = (line + 5).min(lines.len());
        let surrounding_lines = lines[start_line..end_line].to_vec();

        // Analyze current scope
        let current_function = self.analyze_current_function(&source_code, line)?;
        let current_class = self.analyze_current_class(&source_code, line)?;

        let cursor_context = CursorContext {
            line,
            column,
            selected_text,
            surrounding_lines,
            current_function,
            current_class,
        };

        let mut context = self.context.write().await;
        context.cursor_context = Some(cursor_context);
        context.current_file = Some(file_path.to_path_buf());

        // Update recent files
        self.update_recent_files(&mut context, file_path);

        Ok(())
    }

    /// Analyze workspace structure and build project understanding
    async fn analyze_workspace_structure(&mut self, workspace_root: &Path) -> Result<()> {
        // Analyze project structure
        let mut project_structure = HashMap::new();

        for entry in walkdir::WalkDir::new(workspace_root) {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy().to_string();
                    *project_structure.entry(ext_str).or_insert(0) += 1;
                }
            }
        }

        // Detect project type and framework
        let project_type = self.detect_project_type(&project_structure)?;
        let frameworks = self.detect_frameworks(workspace_root)?;

        // Update context with project information
        let mut context = self.context.write().await;
        context.symbol_table.insert(
            "project_info".to_string(),
            SymbolInfo {
                name: "project_info".to_string(),
                kind: SymbolKind::Module,
                location: Location {
                    file: workspace_root.to_string_lossy().to_string(),
                    line: 0,
                    column: 0,
                },
                definition: format!(
                    "{:?} project with frameworks: {:?}",
                    project_type, frameworks
                ),
                usages: Vec::new(),
                related_symbols: Vec::new(),
                confidence_score: 0.9,
            },
        );

        Ok(())
    }

    /// Build comprehensive symbol table
    async fn build_symbol_table(&mut self, workspace_root: &Path) -> Result<()> {
        for entry in walkdir::WalkDir::new(workspace_root) {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && self.is_supported_file(path) {
                if let Ok(source_code) = tokio::fs::read_to_string(path).await {
                    let analysis = self
                        .analyzer
                        .analyze_file_with_tree_sitter(path, &source_code)?;

                    // Extract symbols from analysis
                    for symbol in &analysis.symbols {
                        let symbol_info = SymbolInfo {
                            name: symbol.name.clone(),
                            kind: self
                                .map_symbol_kind(&format!("{:?}", symbol.kind).to_lowercase()),
                            location: Location {
                                file: path.to_string_lossy().to_string(),
                                line: symbol.position.row,
                                column: symbol.position.column,
                            },
                            definition: symbol
                                .signature
                                .clone()
                                .unwrap_or_else(|| symbol.name.clone()),
                            usages: Vec::new(), // Would be populated by cross-reference analysis
                            related_symbols: Vec::new(),
                            confidence_score: 0.8,
                        };

                        self.context
                            .write()
                            .await
                            .symbol_table
                            .insert(symbol.name.clone(), symbol_info);
                    }
                }
            }
        }

        Ok(())
    }

    /// Update dependency relationships
    async fn update_dependency_graph(&mut self) -> Result<()> {
        let context = self.context.read().await;
        let mut dependency_updates = HashMap::new();

        // Analyze import relationships
        for (symbol_name, symbol_info) in &context.symbol_table {
            let mut dependencies = Vec::new();

            // Extract dependencies from definition
            if let Some(imports) = self.extract_imports(&symbol_info.definition) {
                dependencies.extend(imports);
            }

            dependency_updates.insert(symbol_name.clone(), dependencies);
        }

        // Update the context
        let mut context = self.context.write().await;
        for (symbol_name, dependencies) in dependency_updates {
            context.dependency_graph.insert(symbol_name, dependencies);
        }

        Ok(())
    }

    /// Helper methods
    fn is_supported_file(&self, path: &Path) -> bool {
        if let Some(ext) = path.extension() {
            matches!(
                ext.to_str(),
                Some("rs") | Some("py") | Some("js") | Some("ts") | Some("go") | Some("java")
            )
        } else {
            false
        }
    }

    fn map_symbol_kind(&self, kind: &str) -> SymbolKind {
        match kind {
            "function" => SymbolKind::Function,
            "class" => SymbolKind::Class,
            "struct" => SymbolKind::Struct,
            "enum" => SymbolKind::Enum,
            "variable" => SymbolKind::Variable,
            "constant" => SymbolKind::Constant,
            "module" => SymbolKind::Module,
            "type" => SymbolKind::Type,
            "macro" => SymbolKind::Macro,
            _ => SymbolKind::Variable,
        }
    }

    fn detect_project_type(&self, structure: &HashMap<String, usize>) -> Result<String> {
        // Simple project type detection based on file extensions
        if structure.contains_key("rs") {
            Ok("Rust".to_string())
        } else if structure.contains_key("py") {
            Ok("Python".to_string())
        } else if structure.contains_key("js") || structure.contains_key("ts") {
            Ok("JavaScript/TypeScript".to_string())
        } else if structure.contains_key("go") {
            Ok("Go".to_string())
        } else if structure.contains_key("java") {
            Ok("Java".to_string())
        } else {
            Ok("Unknown".to_string())
        }
    }

    fn detect_frameworks(&self, workspace_root: &Path) -> Result<Vec<String>> {
        let mut frameworks = Vec::new();

        // Check for common framework indicators
        let framework_indicators = [
            ("Cargo.toml", "Rust"),
            ("package.json", "Node.js"),
            ("requirements.txt", "Python"),
            ("go.mod", "Go"),
            ("pom.xml", "Maven"),
            ("build.gradle", "Gradle"),
        ];

        for (file, framework) in &framework_indicators {
            if workspace_root.join(file).exists() {
                frameworks.push(framework.to_string());
            }
        }

        Ok(frameworks)
    }

    fn analyze_current_function(&self, source_code: &str, line: usize) -> Result<Option<String>> {
        // Simple function detection - would be enhanced with tree-sitter
        let lines: Vec<&str> = source_code.lines().collect();
        if line >= lines.len() {
            return Ok(None);
        }

        // Look backwards for function definition
        for i in (0..=line).rev() {
            let line_content = lines[i].trim();
            if line_content.starts_with("fn ") {
                if let Some(end) = line_content.find('(') {
                    return Ok(Some(line_content[3..end].trim().to_string()));
                }
            }
        }

        Ok(None)
    }

    fn analyze_current_class(&self, source_code: &str, line: usize) -> Result<Option<String>> {
        // Simple class/struct detection
        let lines: Vec<&str> = source_code.lines().collect();
        if line >= lines.len() {
            return Ok(None);
        }

        // Look backwards for class/struct definition
        for i in (0..=line).rev() {
            let line_content = lines[i].trim();
            if line_content.starts_with("struct ") || line_content.starts_with("class ") {
                let parts: Vec<&str> = line_content.split_whitespace().collect();
                if parts.len() >= 2 {
                    return Ok(Some(parts[1].to_string()));
                }
            }
        }

        Ok(None)
    }

    fn extract_imports(&self, definition: &str) -> Option<Vec<String>> {
        // Simple import extraction - would be enhanced with language-specific parsing
        let mut imports = Vec::new();

        for line in definition.lines() {
            let line = line.trim();
            if line.starts_with("use ") || line.starts_with("import ") || line.starts_with("from ")
            {
                imports.push(line.to_string());
            }
        }

        if imports.is_empty() {
            None
        } else {
            Some(imports)
        }
    }

    fn update_recent_files(&self, context: &mut SemanticContext, file_path: &Path) {
        let path_buf = file_path.to_path_buf();

        // Remove if already exists
        context.recent_files.retain(|p| p != &path_buf);

        // Add to front
        context.recent_files.insert(0, path_buf);

        // Keep only last 10
        if context.recent_files.len() > 10 {
            context.recent_files.truncate(10);
        }
    }
}

/// Pattern learning system
pub struct PatternLearner {
    patterns: Arc<RwLock<HashMap<String, CodePattern>>>,
}

impl PatternLearner {
    pub fn new() -> Self {
        Self {
            patterns: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn learn_patterns(&self, context: &SemanticContext) -> Result<()> {
        let mut patterns = self.patterns.write().await;

        // Analyze naming conventions
        self.analyze_naming_conventions(context, &mut patterns)
            .await?;

        // Analyze error handling patterns
        self.analyze_error_patterns(context, &mut patterns).await?;

        Ok(())
    }

    async fn analyze_naming_conventions(
        &self,
        context: &SemanticContext,
        patterns: &mut HashMap<String, CodePattern>,
    ) -> Result<()> {
        let mut snake_case_count = 0;
        let mut camel_case_count = 0;
        let mut pascal_case_count = 0;

        for symbol in context.symbol_table.values() {
            if symbol.name.contains('_') {
                snake_case_count += 1;
            } else if symbol
                .name
                .chars()
                .next()
                .map_or(false, |c| c.is_lowercase())
            {
                camel_case_count += 1;
            } else {
                pascal_case_count += 1;
            }
        }

        let total = snake_case_count + camel_case_count + pascal_case_count;
        if total > 0 {
            let dominant_style =
                if snake_case_count > camel_case_count && snake_case_count > pascal_case_count {
                    "snake_case"
                } else if camel_case_count > pascal_case_count {
                    "camelCase"
                } else {
                    "PascalCase"
                };

            patterns.insert(
                "naming_convention".to_string(),
                CodePattern {
                    pattern_type: PatternType::NamingConvention,
                    description: format!("Dominant naming convention: {}", dominant_style),
                    examples: vec![
                        "snake_case".to_string(),
                        "camelCase".to_string(),
                        "PascalCase".to_string(),
                    ],
                    frequency: total,
                    confidence: 0.8,
                },
            );
        }

        Ok(())
    }

    async fn analyze_error_patterns(
        &self,
        context: &SemanticContext,
        patterns: &mut HashMap<String, CodePattern>,
    ) -> Result<()> {
        // Analyze error handling patterns in the codebase
        let mut error_patterns = Vec::new();

        for symbol in context.symbol_table.values() {
            if symbol.definition.contains("Result<") || symbol.definition.contains("Option<") {
                error_patterns.push(symbol.name.clone());
            }
        }

        if !error_patterns.is_empty() {
            patterns.insert(
                "error_handling".to_string(),
                CodePattern {
                    pattern_type: PatternType::ErrorHandling,
                    description: "Uses Result/Option types for error handling".to_string(),
                    examples: error_patterns.clone().into_iter().take(3).collect(),
                    frequency: error_patterns.len(),
                    confidence: 0.9,
                },
            );
        }

        Ok(())
    }
}

/// Intelligent code completion engine
pub struct CompletionEngine {
    // Would contain ML models or rule-based systems for completion
}

impl CompletionEngine {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn generate_completions(
        &self,
        context: &SemanticContext,
        _analysis: &CodeAnalysis,
        _line: usize,
        _column: usize,
        _prefix: &str,
    ) -> Result<Vec<CompletionSuggestion>> {
        let mut suggestions = Vec::new();

        // Generate symbol-based completions
        self.generate_symbol_completions(context, _prefix, &mut suggestions)
            .await?;

        // Generate context-aware completions
        self.generate_context_completions(
            context,
            _analysis,
            _line,
            _column,
            _prefix,
            &mut suggestions,
        )
        .await?;

        // Generate pattern-based completions
        self.generate_pattern_completions(context, _prefix, &mut suggestions)
            .await?;

        // Sort by relevance
        suggestions.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap());

        Ok(suggestions.into_iter().take(10).collect())
    }

    async fn generate_symbol_completions(
        &self,
        context: &SemanticContext,
        prefix: &str,
        suggestions: &mut Vec<CompletionSuggestion>,
    ) -> Result<()> {
        for (name, symbol) in &context.symbol_table {
            if name.starts_with(prefix) {
                suggestions.push(CompletionSuggestion {
                    text: name.clone(),
                    kind: match symbol.kind {
                        SymbolKind::Function => CompletionKind::Function,
                        SymbolKind::Variable => CompletionKind::Variable,
                        SymbolKind::Type => CompletionKind::Type,
                        _ => CompletionKind::Variable,
                    },
                    relevance_score: 0.8,
                    context_info: format!("{:?} - {}", symbol.kind, symbol.location.file),
                    documentation: Some(symbol.definition.clone()),
                });
            }
        }

        Ok(())
    }

    async fn generate_context_completions(
        &self,
        context: &SemanticContext,
        _analysis: &CodeAnalysis,
        _line: usize,
        _column: usize,
        _prefix: &str,
        suggestions: &mut Vec<CompletionSuggestion>,
    ) -> Result<()> {
        // Context-aware completions based on current location
        if let Some(cursor_ctx) = &context.cursor_context {
            if let Some(current_function) = &cursor_ctx.current_function {
                // Suggest local variables or parameters
                suggestions.push(CompletionSuggestion {
                    text: format!("{}_result", current_function.to_lowercase()),
                    kind: CompletionKind::Variable,
                    relevance_score: 0.7,
                    context_info: "Local variable suggestion".to_string(),
                    documentation: None,
                });
            }
        }

        Ok(())
    }

    async fn generate_pattern_completions(
        &self,
        context: &SemanticContext,
        prefix: &str,
        suggestions: &mut Vec<CompletionSuggestion>,
    ) -> Result<()> {
        // Generate completions based on learned patterns
        for pattern in &context.code_patterns {
            match pattern.pattern_type {
                PatternType::NamingConvention => {
                    if pattern.description.contains("snake_case") && prefix.contains('_') {
                        suggestions.push(CompletionSuggestion {
                            text: format!("{}_value", prefix),
                            kind: CompletionKind::Variable,
                            relevance_score: 0.6,
                            context_info: "Following naming convention".to_string(),
                            documentation: Some(pattern.description.clone()),
                        });
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }
}
