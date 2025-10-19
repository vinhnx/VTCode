//! Code analysis capabilities using tree-sitter

use crate::tools::tree_sitter::analyzer::{LanguageSupport, Position, SyntaxNode, SyntaxTree};
use crate::tools::tree_sitter::languages::{LanguageAnalyzer, SymbolInfo, SymbolKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Comprehensive code analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAnalysis {
    pub file_path: String,
    pub language: LanguageSupport,
    pub metrics: CodeMetrics,
    pub symbols: Vec<SymbolInfo>,
    pub dependencies: Vec<DependencyInfo>,
    pub issues: Vec<AnalysisIssue>,
    pub complexity: ComplexityMetrics,
    pub structure: CodeStructure,
}

/// Code metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeMetrics {
    pub lines_of_code: usize,
    pub lines_of_comments: usize,
    pub blank_lines: usize,
    pub functions_count: usize,
    pub classes_count: usize,
    pub variables_count: usize,
    pub imports_count: usize,
    pub comment_ratio: f64,
}

/// Dependency information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyInfo {
    pub name: String,
    pub kind: DependencyKind,
    pub source: String,
    pub position: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DependencyKind {
    Import,
    Package,
    Module,
    External,
}

/// Analysis issues and suggestions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisIssue {
    pub level: IssueLevel,
    pub category: IssueCategory,
    pub message: String,
    pub position: Position,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IssueLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IssueCategory {
    Style,
    Performance,
    Security,
    Complexity,
    Maintainability,
}

/// Code complexity metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComplexityMetrics {
    pub cyclomatic_complexity: usize,
    pub cognitive_complexity: usize,
    pub nesting_depth: usize,
    pub function_length_average: f64,
    pub function_length_max: usize,
    pub parameters_average: f64,
    pub parameters_max: usize,
}

/// Code structure information
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeStructure {
    pub modules: Vec<String>,
    pub functions: Vec<String>,
    pub classes: Vec<String>,
    pub hierarchy: HashMap<String, Vec<String>>, // parent -> children
}

/// Code analyzer using tree-sitter
pub struct CodeAnalyzer {
    language_analyzer: LanguageAnalyzer,
}

impl CodeAnalyzer {
    pub fn new(language: &LanguageSupport) -> Self {
        Self {
            language_analyzer: LanguageAnalyzer::new(language),
        }
    }

    /// Perform comprehensive code analysis
    pub fn analyze(&self, syntax_tree: &SyntaxTree, file_path: &str) -> CodeAnalysis {
        let symbols = self.language_analyzer.extract_symbols(syntax_tree);
        let metrics = self.calculate_metrics(syntax_tree, &symbols);
        let dependencies = self.extract_dependencies(syntax_tree);
        let issues = self.analyze_issues(syntax_tree, &symbols);
        let complexity = self.calculate_complexity(syntax_tree, &symbols);
        let structure = self.analyze_structure(&symbols);

        CodeAnalysis {
            file_path: file_path.to_string(),
            language: syntax_tree.language,
            metrics,
            symbols,
            dependencies,
            issues,
            complexity,
            structure,
        }
    }

    /// Calculate basic code metrics
    fn calculate_metrics(&self, tree: &SyntaxTree, symbols: &[SymbolInfo]) -> CodeMetrics {
        let lines = tree.source_code.lines().collect::<Vec<_>>();
        let total_lines = lines.len();

        let mut comment_lines = 0;
        let mut blank_lines = 0;

        for line in &lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                blank_lines += 1;
            } else if trimmed.starts_with("//")
                || trimmed.starts_with("/*")
                || trimmed.starts_with("#")
                || trimmed.starts_with("'''")
            {
                comment_lines += 1;
            }
        }

        let code_lines = total_lines - comment_lines - blank_lines;
        let comment_ratio = if code_lines > 0 {
            comment_lines as f64 / code_lines as f64
        } else {
            0.0
        };

        let functions_count = symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
            .count();
        let classes_count = symbols
            .iter()
            .filter(|s| {
                matches!(
                    s.kind,
                    SymbolKind::Class | SymbolKind::Struct | SymbolKind::Interface
                )
            })
            .count();
        let variables_count = symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Variable))
            .count();
        let imports_count = symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Import))
            .count();

        CodeMetrics {
            lines_of_code: code_lines,
            lines_of_comments: comment_lines,
            blank_lines,
            functions_count,
            classes_count,
            variables_count,
            imports_count,
            comment_ratio,
        }
    }

    /// Extract dependencies from the code
    fn extract_dependencies(&self, tree: &SyntaxTree) -> Vec<DependencyInfo> {
        let mut dependencies = Vec::new();

        // Extract imports based on language
        match tree.language {
            LanguageSupport::Rust => {
                self.extract_rust_dependencies(&tree.root, &mut dependencies);
            }
            LanguageSupport::Python => {
                self.extract_python_dependencies(&tree.root, &mut dependencies);
            }
            LanguageSupport::JavaScript | LanguageSupport::TypeScript => {
                self.extract_js_dependencies(&tree.root, &mut dependencies);
            }
            LanguageSupport::Go => {
                self.extract_go_dependencies(&tree.root, &mut dependencies);
            }
            LanguageSupport::Java => {
                self.extract_java_dependencies(&tree.root, &mut dependencies);
            }
            LanguageSupport::Bash => {
                self.extract_bash_dependencies(&tree.root, &mut dependencies);
            }
            LanguageSupport::Swift => {
                self.extract_swift_dependencies(&tree.root, &mut dependencies);
            }
        }

        dependencies
    }

    fn extract_bash_dependencies(&self, node: &SyntaxNode, deps: &mut Vec<DependencyInfo>) {
        if node.kind == "command" {
            let trimmed = node.text.trim_start();

            let (prefix, path) = if let Some(rest) = trimmed.strip_prefix("source ") {
                ("source", rest.trim())
            } else if let Some(rest) = trimmed.strip_prefix(". ") {
                (".", rest.trim())
            } else {
                ("", "")
            };

            if !path.is_empty() {
                deps.push(DependencyInfo {
                    name: path.to_string(),
                    kind: DependencyKind::Import,
                    source: prefix.to_string(),
                    position: node.start_position.clone(),
                });
            }
        }

        for child in &node.children {
            self.extract_bash_dependencies(child, deps);
        }
    }

    fn extract_rust_dependencies(&self, node: &SyntaxNode, deps: &mut Vec<DependencyInfo>) {
        if node.kind == "use_declaration"
            && let Some(path_node) = node
                .named_children
                .get("argument")
                .and_then(|children| children.first())
        {
            deps.push(DependencyInfo {
                name: path_node.text.clone(),
                kind: DependencyKind::Import,
                source: "use".to_string(),
                position: path_node.start_position.clone(),
            });
        }

        for child in &node.children {
            self.extract_rust_dependencies(child, deps);
        }
    }

    fn extract_python_dependencies(&self, node: &SyntaxNode, deps: &mut Vec<DependencyInfo>) {
        if node.kind == "import_statement" || node.kind == "import_from_statement" {
            for child in &node.children {
                if child.kind == "dotted_name" {
                    deps.push(DependencyInfo {
                        name: child.text.clone(),
                        kind: DependencyKind::Import,
                        source: "import".to_string(),
                        position: child.start_position.clone(),
                    });
                }
            }
        }

        for child in &node.children {
            self.extract_python_dependencies(child, deps);
        }
    }

    fn extract_js_dependencies(&self, node: &SyntaxNode, deps: &mut Vec<DependencyInfo>) {
        if node.kind == "import_statement" {
            for child in &node.children {
                if child.kind == "string" {
                    deps.push(DependencyInfo {
                        name: child.text.clone(),
                        kind: DependencyKind::Import,
                        source: "import".to_string(),
                        position: child.start_position.clone(),
                    });
                }
            }
        }

        for child in &node.children {
            self.extract_js_dependencies(child, deps);
        }
    }

    fn extract_go_dependencies(&self, node: &SyntaxNode, deps: &mut Vec<DependencyInfo>) {
        if node.kind == "import_declaration" {
            for child in &node.children {
                if let Some(spec_node) = child.named_children.get("spec") {
                    if let Some(path_node) =
                        spec_node.first().and_then(|n| n.named_children.get("path"))
                    {
                        if let Some(string_node) = path_node.first() {
                            deps.push(DependencyInfo {
                                name: string_node.text.clone(),
                                kind: DependencyKind::Import,
                                source: "import".to_string(),
                                position: string_node.start_position.clone(),
                            });
                        }
                    }
                }
            }
        }

        for child in &node.children {
            self.extract_go_dependencies(child, deps);
        }
    }

    fn extract_java_dependencies(&self, node: &SyntaxNode, deps: &mut Vec<DependencyInfo>) {
        if node.kind == "import_declaration" {
            for child in &node.children {
                if let Some(name_node) = child.named_children.get("qualified_name") {
                    if let Some(name) = name_node.first() {
                        deps.push(DependencyInfo {
                            name: name.text.clone(),
                            kind: DependencyKind::Import,
                            source: "import".to_string(),
                            position: name.start_position.clone(),
                        });
                    }
                }
            }
        }

        for child in &node.children {
            self.extract_java_dependencies(child, deps);
        }
    }

    #[allow(dead_code)]
    fn extract_swift_dependencies(&self, node: &SyntaxNode, deps: &mut Vec<DependencyInfo>) {
        if node.kind == "import_declaration" {
            for child in &node.children {
                if let Some(path_node) = child.named_children.get("path") {
                    if let Some(path) = path_node.first() {
                        deps.push(DependencyInfo {
                            name: path.text.clone(),
                            kind: DependencyKind::Import,
                            source: "import".to_string(),
                            position: path.start_position.clone(),
                        });
                    }
                }
            }
        }

        for child in &node.children {
            self.extract_swift_dependencies(child, deps);
        }
    }

    /// Analyze code for potential issues
    fn analyze_issues(&self, tree: &SyntaxTree, symbols: &[SymbolInfo]) -> Vec<AnalysisIssue> {
        let mut issues = Vec::new();

        // Check for long functions
        for symbol in symbols {
            if matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method) {
                if let Some(signature) = &symbol.signature {
                    if signature.len() > 100 {
                        issues.push(AnalysisIssue {
                            level: IssueLevel::Info,
                            category: IssueCategory::Maintainability,
                            message: format!("Long function signature: {}", symbol.name),
                            position: symbol.position.clone(),
                            suggestion: Some(
                                "Consider breaking down into smaller functions".to_string(),
                            ),
                        });
                    }
                }
            }
        }

        // Check for high cyclomatic complexity (simplified)
        let complexity = self.calculate_complexity(tree, symbols);
        if complexity.cyclomatic_complexity > 10 {
            issues.push(AnalysisIssue {
                level: IssueLevel::Warning,
                category: IssueCategory::Complexity,
                message: format!(
                    "High cyclomatic complexity: {}",
                    complexity.cyclomatic_complexity
                ),
                position: Position {
                    row: 0,
                    column: 0,
                    byte_offset: 0,
                },
                suggestion: Some("Consider refactoring to reduce complexity".to_string()),
            });
        }

        // Check for missing documentation
        for symbol in symbols {
            if matches!(symbol.kind, SymbolKind::Function | SymbolKind::Class)
                && symbol.documentation.is_none()
            {
                issues.push(AnalysisIssue {
                    level: IssueLevel::Info,
                    category: IssueCategory::Maintainability,
                    message: format!("Missing documentation for: {}", symbol.name),
                    position: symbol.position.clone(),
                    suggestion: Some("Add documentation comments".to_string()),
                });
            }
        }

        issues
    }

    /// Calculate code complexity metrics
    fn calculate_complexity(&self, tree: &SyntaxTree, symbols: &[SymbolInfo]) -> ComplexityMetrics {
        let mut cyclomatic_complexity = 1; // Base complexity
        let mut cognitive_complexity = 0;
        let mut max_nesting_depth = 0;

        // Calculate complexity based on language-specific constructs
        self.calculate_language_complexity(
            &tree.root,
            &mut cyclomatic_complexity,
            &mut cognitive_complexity,
            0,
            &mut max_nesting_depth,
        );

        let function_lengths: Vec<usize> = symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
            .filter_map(|s| s.signature.as_ref().map(|sig| sig.lines().count()))
            .collect();

        let function_length_average = if !function_lengths.is_empty() {
            function_lengths.iter().sum::<usize>() as f64 / function_lengths.len() as f64
        } else {
            0.0
        };

        let function_length_max = function_lengths.iter().cloned().max().unwrap_or(0);

        // Calculate parameter statistics
        let parameter_counts: Vec<usize> = symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
            .filter_map(|s| s.signature.as_ref())
            .filter_map(|sig| {
                let start = sig.find('(')?;
                let end = sig.find(')')?;
                let params = &sig[start + 1..end];
                Some(params.split(',').filter(|p| !p.trim().is_empty()).count())
            })
            .collect();

        let parameters_average = if !parameter_counts.is_empty() {
            parameter_counts.iter().sum::<usize>() as f64 / parameter_counts.len() as f64
        } else {
            0.0
        };

        let parameters_max = parameter_counts.iter().cloned().max().unwrap_or(0);

        ComplexityMetrics {
            cyclomatic_complexity,
            cognitive_complexity,
            nesting_depth: max_nesting_depth,
            function_length_average,
            function_length_max,
            parameters_average,
            parameters_max,
        }
    }

    fn calculate_language_complexity(
        &self,
        node: &SyntaxNode,
        cc: &mut usize,
        cognitive: &mut usize,
        depth: usize,
        max_depth: &mut usize,
    ) {
        *max_depth = (*max_depth).max(depth);

        // Language-specific complexity calculations
        match node.kind.as_str() {
            // Control flow increases cyclomatic complexity
            k if k.contains("if") || k.contains("else") => {
                *cc += 1;
                *cognitive += 1;
            }
            k if k.contains("for") || k.contains("while") || k.contains("loop") => {
                *cc += 1;
                *cognitive += 2;
            }
            k if k.contains("switch") || k.contains("match") => {
                *cc += node
                    .named_children
                    .get("body")
                    .and_then(|children| Some(children.len().saturating_sub(1)))
                    .unwrap_or(0);
                *cognitive += 1;
            }
            k if k.contains("try") || k.contains("catch") => {
                *cc += 1;
                *cognitive += 1;
            }
            k if k.contains("function") || k.contains("method") => {
                *cognitive += 1; // Function definition
            }
            _ => {}
        }

        // Recursively calculate for children
        for child in &node.children {
            self.calculate_language_complexity(child, cc, cognitive, depth + 1, max_depth);
        }
    }

    /// Analyze code structure
    fn analyze_structure(&self, symbols: &[SymbolInfo]) -> CodeStructure {
        let mut modules = Vec::new();
        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut hierarchy = HashMap::new();

        for symbol in symbols {
            match &symbol.kind {
                SymbolKind::Module => modules.push(symbol.name.clone()),
                SymbolKind::Function => functions.push(symbol.name.clone()),
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::Interface => {
                    classes.push(symbol.name.clone());
                }
                _ => {}
            }

            // Build hierarchy (simplified - in practice, this would be more sophisticated)
            if let Some(scope) = &symbol.scope {
                hierarchy
                    .entry(scope.clone())
                    .or_insert_with(Vec::new)
                    .push(symbol.name.clone());
            }
        }

        CodeStructure {
            modules,
            functions,
            classes,
            hierarchy,
        }
    }
}

/// Utility functions for code analysis
pub struct AnalysisUtils;

impl AnalysisUtils {
    /// Calculate code duplication (simplified)
    pub fn calculate_duplication(tree: &SyntaxTree) -> f64 {
        // Implement a more sophisticated duplication analysis
        // This looks for similar code structures in the tree

        // Traverse the tree and count nodes
        fn traverse_node(
            node: &SyntaxNode,
            node_counts: &mut std::collections::HashMap<String, usize>,
        ) -> usize {
            let mut count = 1; // Count this node

            // Create a signature for this node based on its kind and children
            let mut signature = node.kind.clone();

            // Add children signatures
            for child in &node.children {
                let child_count = traverse_node(child, node_counts);
                count += child_count;
                signature.push_str(&format!("_{}", child.kind));
            }

            // Update the count for this signature
            *node_counts.entry(signature).or_insert(0) += 1;

            count
        }

        let mut node_counts = std::collections::HashMap::new();
        let total_nodes = traverse_node(&tree.root, &mut node_counts);

        // Count how many patterns appear more than once
        let duplicate_patterns = node_counts.values().filter(|&&count| count > 1).count();

        // Calculate duplication ratio
        if total_nodes == 0 {
            0.0
        } else {
            (duplicate_patterns as f64 / total_nodes as f64) * 100.0
        }
    }

    /// Analyze code maintainability index
    pub fn calculate_maintainability_index(analysis: &CodeAnalysis) -> f64 {
        let metrics = &analysis.metrics;
        let complexity = &analysis.complexity;

        // Simplified maintainability index calculation
        let halstead_volume =
            metrics.lines_of_code as f64 * (metrics.functions_count as f64).log2();
        let cyclomatic_complexity = complexity.cyclomatic_complexity as f64;
        let lines_of_code = metrics.lines_of_code as f64;

        let mi = 171.0
            - 5.2 * halstead_volume.log2()
            - 0.23 * cyclomatic_complexity
            - 16.2 * lines_of_code.log2();
        mi.max(0.0).min(171.0) // Clamp between 0 and 171
    }

    /// Generate code quality score
    pub fn calculate_quality_score(analysis: &CodeAnalysis) -> f64 {
        let mut score: f64 = 100.0;

        // Deduct points for issues
        for issue in &analysis.issues {
            match issue.level {
                IssueLevel::Error => score -= 20.0,
                IssueLevel::Warning => score -= 10.0,
                IssueLevel::Info => score -= 2.0,
            }
        }

        // Bonus for good practices
        if analysis.metrics.comment_ratio > 0.1 {
            score += 5.0;
        }

        if analysis.complexity.cyclomatic_complexity < 5 {
            score += 10.0;
        }

        score.max(0.0).min(100.0)
    }
}
