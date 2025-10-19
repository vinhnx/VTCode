//! Core tree-sitter analyzer for code parsing and analysis

use crate::tools::tree_sitter::analysis::{
    CodeAnalysis, CodeMetrics, DependencyInfo, DependencyKind,
};
use crate::tools::tree_sitter::languages::*;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Language, Parser, Tree};

/// Tree-sitter analysis error
#[derive(Debug, thiserror::Error)]
pub enum TreeSitterError {
    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("File read error: {0}")]
    FileReadError(String),

    #[error("Language detection failed: {0}")]
    LanguageDetectionError(String),

    #[error("Query execution error: {0}")]
    QueryError(String),
}

/// Language support enumeration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, Hash, PartialEq)]
pub enum LanguageSupport {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    Bash,
    Swift,
}

/// Syntax tree representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntaxTree {
    pub root: SyntaxNode,
    pub source_code: String,
    pub language: LanguageSupport,
    pub diagnostics: Vec<Diagnostic>,
}

/// Syntax node in the tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntaxNode {
    pub kind: String,
    pub start_position: Position,
    pub end_position: Position,
    pub text: String,
    // Children within the AST subtree
    pub children: Vec<SyntaxNode>,
    pub named_children: HashMap<String, Vec<SyntaxNode>>,
    // Collected comments that immediately precede this node as sibling comments
    // (useful for documentation extraction like docstrings or /// comments)
    pub leading_comments: Vec<String>,
}

/// Position in source code
#[derive(Debug, Clone, Serialize, Deserialize, Eq, Hash, PartialEq)]
pub struct Position {
    pub row: usize,
    pub column: usize,
    pub byte_offset: usize,
}

/// Diagnostic information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
    pub position: Position,
    pub node_kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DiagnosticLevel {
    Error,
    Warning,
    Info,
}

/// Main tree-sitter analyzer
pub struct TreeSitterAnalyzer {
    parsers: HashMap<LanguageSupport, Parser>,
    supported_languages: Vec<LanguageSupport>,
    current_file: String,
}

impl TreeSitterAnalyzer {
    /// Create a new tree-sitter analyzer
    pub fn new() -> Result<Self> {
        let mut parsers = HashMap::new();

        // Initialize parsers for all supported languages
        let mut languages = vec![
            LanguageSupport::Rust,
            LanguageSupport::Python,
            LanguageSupport::JavaScript,
            LanguageSupport::TypeScript,
            LanguageSupport::Go,
            LanguageSupport::Java,
            LanguageSupport::Bash,
        ];

        if cfg!(feature = "swift") {
            // Swift grammar provided by https://github.com/tree-sitter/swift-tree-sitter via the tree-sitter-swift crate
            languages.push(LanguageSupport::Swift);
        }

        for language in &languages {
            let mut parser = Parser::new();
            let ts_language = get_language(language.clone())?;
            parser.set_language(&ts_language)?;
            parsers.insert(language.clone(), parser);
        }

        Ok(Self {
            parsers,
            supported_languages: languages,
            current_file: String::new(),
        })
    }

    /// Get supported languages
    pub fn supported_languages(&self) -> &[LanguageSupport] {
        &self.supported_languages
    }

    /// Detect language from file extension
    pub fn detect_language_from_path<P: AsRef<Path>>(&self, path: P) -> Result<LanguageSupport> {
        let path = path.as_ref();
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .ok_or_else(|| {
                TreeSitterError::LanguageDetectionError("No file extension found".to_string())
            })?;

        let normalized_extension = extension.to_ascii_lowercase();

        match normalized_extension.as_str() {
            "rs" => Ok(LanguageSupport::Rust),
            "py" => Ok(LanguageSupport::Python),
            "js" => Ok(LanguageSupport::JavaScript),
            "ts" => Ok(LanguageSupport::TypeScript),
            "tsx" => Ok(LanguageSupport::TypeScript),
            "jsx" => Ok(LanguageSupport::JavaScript),
            "go" => Ok(LanguageSupport::Go),
            "java" => Ok(LanguageSupport::Java),
            "sh" | "bash" => Ok(LanguageSupport::Bash),
            "swift" => {
                if cfg!(feature = "swift") {
                    Ok(LanguageSupport::Swift)
                } else {
                    Err(TreeSitterError::UnsupportedLanguage("Swift".to_string()).into())
                }
            }
            _ => Err(TreeSitterError::UnsupportedLanguage(extension.to_string()).into()),
        }
    }

    /// Parse source code into a syntax tree
    pub fn parse(&mut self, source_code: &str, language: LanguageSupport) -> Result<Tree> {
        let parser = self
            .parsers
            .get_mut(&language)
            .ok_or_else(|| TreeSitterError::UnsupportedLanguage(format!("{:?}", language)))?;

        let tree = parser.parse(source_code, None).ok_or_else(|| {
            TreeSitterError::ParseError("Failed to parse source code".to_string())
        })?;

        Ok(tree)
    }

    /// Extract symbols from a syntax tree
    pub fn extract_symbols(
        &mut self,
        syntax_tree: &Tree,
        source_code: &str,
        language: LanguageSupport,
    ) -> Result<Vec<SymbolInfo>> {
        let mut symbols = Vec::new();
        let root_node = syntax_tree.root_node();

        // Walk the tree and extract symbols based on language
        self.extract_symbols_recursive(root_node, source_code, language, &mut symbols, None)?;

        Ok(symbols)
    }

    /// Recursively extract symbols from a node
    fn extract_symbols_recursive(
        &self,
        node: tree_sitter::Node,
        source_code: &str,
        language: LanguageSupport,
        symbols: &mut Vec<SymbolInfo>,
        parent_scope: Option<String>,
    ) -> Result<()> {
        let _node_text = &source_code[node.start_byte()..node.end_byte()];
        let kind = node.kind();

        // Extract symbols based on node type and language
        match language {
            LanguageSupport::Rust => {
                if kind == "function_item" || kind == "method_definition" {
                    // Extract function name
                    if let Some(name_node) = self.find_child_by_type(node, "identifier") {
                        let name = &source_code[name_node.start_byte()..name_node.end_byte()];
                        symbols.push(SymbolInfo {
                            name: name.to_string(),
                            kind: SymbolKind::Function,
                            position: Position {
                                row: node.start_position().row,
                                column: node.start_position().column,
                                byte_offset: node.start_byte(),
                            },
                            scope: parent_scope.clone(),
                            signature: None,
                            documentation: None,
                        });
                    }
                } else if kind == "struct_item" || kind == "enum_item" {
                    // Extract type name
                    if let Some(name_node) = self.find_child_by_type(node, "type_identifier") {
                        let name = &source_code[name_node.start_byte()..name_node.end_byte()];
                        symbols.push(SymbolInfo {
                            name: name.to_string(),
                            kind: SymbolKind::Type,
                            position: Position {
                                row: node.start_position().row,
                                column: node.start_position().column,
                                byte_offset: node.start_byte(),
                            },
                            scope: parent_scope.clone(),
                            signature: None,
                            documentation: None,
                        });
                    }
                }
            }
            LanguageSupport::Python => {
                if kind == "function_definition" {
                    // Extract function name
                    if let Some(name_node) = self.find_child_by_type(node, "identifier") {
                        let name = &source_code[name_node.start_byte()..name_node.end_byte()];
                        symbols.push(SymbolInfo {
                            name: name.to_string(),
                            kind: SymbolKind::Function,
                            position: Position {
                                row: node.start_position().row,
                                column: node.start_position().column,
                                byte_offset: node.start_byte(),
                            },
                            scope: parent_scope.clone(),
                            signature: None,
                            documentation: None,
                        });
                    }
                } else if kind == "class_definition" {
                    // Extract class name
                    if let Some(name_node) = self.find_child_by_type(node, "identifier") {
                        let name = &source_code[name_node.start_byte()..name_node.end_byte()];
                        symbols.push(SymbolInfo {
                            name: name.to_string(),
                            kind: SymbolKind::Type,
                            position: Position {
                                row: node.start_position().row,
                                column: node.start_position().column,
                                byte_offset: node.start_byte(),
                            },
                            scope: parent_scope.clone(),
                            signature: None,
                            documentation: None,
                        });
                    }
                }
            }
            LanguageSupport::Bash => {
                if kind == "function_definition" {
                    if let Some(name_node) = self.find_child_by_type(node, "word") {
                        let name = &source_code[name_node.start_byte()..name_node.end_byte()];
                        symbols.push(SymbolInfo {
                            name: name.to_string(),
                            kind: SymbolKind::Function,
                            position: Position {
                                row: node.start_position().row,
                                column: node.start_position().column,
                                byte_offset: node.start_byte(),
                            },
                            scope: parent_scope.clone(),
                            signature: None,
                            documentation: None,
                        });
                    }
                } else if kind == "variable_assignment" {
                    if let Some(name_node) = self.find_child_by_type(node, "word") {
                        let name = &source_code[name_node.start_byte()..name_node.end_byte()];
                        symbols.push(SymbolInfo {
                            name: name.to_string(),
                            kind: SymbolKind::Variable,
                            position: Position {
                                row: node.start_position().row,
                                column: node.start_position().column,
                                byte_offset: node.start_byte(),
                            },
                            scope: parent_scope.clone(),
                            signature: None,
                            documentation: None,
                        });
                    }
                }
            }
            _ => {
                // For other languages, do a basic extraction
                if kind.contains("function") || kind.contains("method") {
                    // Try to find a name
                    if let Some(name_node) = self.find_child_by_type(node, "identifier") {
                        let name = &source_code[name_node.start_byte()..name_node.end_byte()];
                        symbols.push(SymbolInfo {
                            name: name.to_string(),
                            kind: SymbolKind::Function,
                            position: Position {
                                row: node.start_position().row,
                                column: node.start_position().column,
                                byte_offset: node.start_byte(),
                            },
                            scope: parent_scope.clone(),
                            signature: None,
                            documentation: None,
                        });
                    }
                }
            }
        }

        // Recursively process children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_symbols_recursive(
                child,
                source_code,
                language.clone(),
                symbols,
                parent_scope.clone(),
            )?;
        }

        Ok(())
    }

    /// Find a child node of a specific type
    fn find_child_by_type<'a>(
        &self,
        node: tree_sitter::Node<'a>,
        type_name: &str,
    ) -> Option<tree_sitter::Node<'a>> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == type_name {
                return Some(child);
            }
        }
        None
    }

    /// Extract dependencies from a syntax tree
    pub fn extract_dependencies(
        &self,
        syntax_tree: &Tree,
        language: LanguageSupport,
    ) -> Result<Vec<DependencyInfo>> {
        let mut dependencies = Vec::new();
        let root_node = syntax_tree.root_node();

        // Extract dependencies based on language
        match language {
            LanguageSupport::Rust => {
                self.extract_rust_dependencies(root_node, &mut dependencies)?;
            }
            LanguageSupport::Python => {
                self.extract_python_dependencies(root_node, &mut dependencies)?;
            }
            LanguageSupport::JavaScript | LanguageSupport::TypeScript => {
                self.extract_js_dependencies(root_node, &mut dependencies)?;
            }
            LanguageSupport::Bash => {
                self.extract_basic_dependencies(root_node, &mut dependencies)?;
            }
            _ => {
                // For other languages, do a basic extraction
                self.extract_basic_dependencies(root_node, &mut dependencies)?;
            }
        }

        Ok(dependencies)
    }

    /// Extract Rust dependencies
    fn extract_rust_dependencies(
        &self,
        node: tree_sitter::Node,
        dependencies: &mut Vec<DependencyInfo>,
    ) -> Result<()> {
        let mut cursor = node.walk();

        // Look for use statements and extern crate declarations
        if node.kind() == "use_declaration" {
            // Extract the path from the use statement
            if let Some(_path_node) = self
                .find_child_by_type(node, "use_list")
                .or_else(|| self.find_child_by_type(node, "scoped_identifier"))
                .or_else(|| self.find_child_by_type(node, "identifier"))
            {
                // This is a simplified extraction
                dependencies.push(DependencyInfo {
                    name: "unknown_rust_dep".to_string(), // Would need more parsing for actual name
                    kind: DependencyKind::Import,
                    source: "use_declaration".to_string(),
                    position: Position {
                        row: node.start_position().row,
                        column: node.start_position().column,
                        byte_offset: node.start_byte(),
                    },
                });
            }
        } else if node.kind() == "extern_crate_declaration" {
            // Extract crate name from extern crate declaration
            if let Some(_name_node) = self.find_child_by_type(node, "identifier") {
                dependencies.push(DependencyInfo {
                    name: "unknown_crate".to_string(), // Would need more parsing for actual name
                    kind: DependencyKind::External,
                    source: "extern_crate".to_string(),
                    position: Position {
                        row: node.start_position().row,
                        column: node.start_position().column,
                        byte_offset: node.start_byte(),
                    },
                });
            }
        }

        // Recursively process children
        for child in node.children(&mut cursor) {
            self.extract_rust_dependencies(child, dependencies)?;
        }

        Ok(())
    }

    /// Extract Python dependencies
    fn extract_python_dependencies(
        &self,
        node: tree_sitter::Node,
        dependencies: &mut Vec<DependencyInfo>,
    ) -> Result<()> {
        let mut cursor = node.walk();

        // Look for import statements
        if node.kind() == "import_statement" || node.kind() == "import_from_statement" {
            // Extract the module name
            dependencies.push(DependencyInfo {
                name: "unknown_python_module".to_string(), // Would need more parsing for actual name
                kind: DependencyKind::Import,
                source: node.kind().to_string(),
                position: Position {
                    row: node.start_position().row,
                    column: node.start_position().column,
                    byte_offset: node.start_byte(),
                },
            });
        }

        // Recursively process children
        for child in node.children(&mut cursor) {
            self.extract_python_dependencies(child, dependencies)?;
        }

        Ok(())
    }

    /// Extract JavaScript/TypeScript dependencies
    fn extract_js_dependencies(
        &self,
        node: tree_sitter::Node,
        dependencies: &mut Vec<DependencyInfo>,
    ) -> Result<()> {
        let mut cursor = node.walk();

        // Look for import statements
        if node.kind() == "import_statement" {
            // Extract the module name
            dependencies.push(DependencyInfo {
                name: "unknown_js_module".to_string(), // Would need more parsing for actual name
                kind: DependencyKind::Import,
                source: node.kind().to_string(),
                position: Position {
                    row: node.start_position().row,
                    column: node.start_position().column,
                    byte_offset: node.start_byte(),
                },
            });
        }

        // Recursively process children
        for child in node.children(&mut cursor) {
            self.extract_js_dependencies(child, dependencies)?;
        }

        Ok(())
    }

    /// Extract basic dependencies (fallback)
    fn extract_basic_dependencies(
        &self,
        node: tree_sitter::Node,
        dependencies: &mut Vec<DependencyInfo>,
    ) -> Result<()> {
        let mut cursor = node.walk();

        // Look for import/include statements
        if node.kind().contains("import") || node.kind().contains("include") {
            // Extract the dependency name
            dependencies.push(DependencyInfo {
                name: "unknown_dependency".to_string(),
                kind: DependencyKind::Import,
                source: node.kind().to_string(),
                position: Position {
                    row: node.start_position().row,
                    column: node.start_position().column,
                    byte_offset: node.start_byte(),
                },
            });
        }

        // Recursively process children
        for child in node.children(&mut cursor) {
            self.extract_basic_dependencies(child, dependencies)?;
        }

        Ok(())
    }

    /// Calculate code metrics from a syntax tree
    pub fn calculate_metrics(&self, syntax_tree: &Tree, source_code: &str) -> Result<CodeMetrics> {
        let root_node = syntax_tree.root_node();
        let lines = source_code.lines().collect::<Vec<_>>();

        // Count different types of nodes
        let mut functions_count = 0;
        let mut classes_count = 0;
        let mut variables_count = 0;
        let mut imports_count = 0;

        self.count_nodes_recursive(
            root_node,
            &mut functions_count,
            &mut classes_count,
            &mut variables_count,
            &mut imports_count,
        );

        // Count comments
        let lines_of_comments = lines
            .iter()
            .filter(|l| {
                l.trim().starts_with("//")
                    || l.trim().starts_with("/*")
                    || l.trim().starts_with("#")
            })
            .count();

        let blank_lines = lines.iter().filter(|l| l.trim().is_empty()).count();
        let lines_of_code = lines.len();

        let comment_ratio = if lines_of_code > 0 {
            lines_of_comments as f64 / lines_of_code as f64
        } else {
            0.0
        };

        Ok(CodeMetrics {
            lines_of_code,
            lines_of_comments,
            blank_lines,
            functions_count,
            classes_count,
            variables_count,
            imports_count,
            comment_ratio,
        })
    }

    /// Recursively count different types of nodes
    fn count_nodes_recursive(
        &self,
        node: tree_sitter::Node,
        functions_count: &mut usize,
        classes_count: &mut usize,
        variables_count: &mut usize,
        imports_count: &mut usize,
    ) {
        let kind = node.kind();

        // Count based on node type
        if kind.contains("function") || kind.contains("method") {
            *functions_count += 1;
        } else if kind.contains("class") || kind.contains("struct") || kind.contains("enum") {
            *classes_count += 1;
        } else if kind.contains("variable")
            || kind.contains("let")
            || kind.contains("const")
            || kind.contains("assignment")
        {
            *variables_count += 1;
        } else if kind.contains("import") || kind.contains("include") || kind.contains("use") {
            *imports_count += 1;
        }

        // Recursively process children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.count_nodes_recursive(
                child,
                functions_count,
                classes_count,
                variables_count,
                imports_count,
            );
        }
    }

    /// Parse file into a syntax tree
    pub fn parse_file<P: AsRef<Path>>(&mut self, file_path: P) -> Result<SyntaxTree> {
        let file_path = file_path.as_ref();
        let language = self.detect_language_from_path(file_path)?;

        let source_code = std::fs::read_to_string(file_path)
            .map_err(|e| TreeSitterError::FileReadError(e.to_string()))?;

        let tree = self.parse(&source_code, language.clone())?;

        // Convert tree-sitter tree to our SyntaxTree representation
        let root = self.convert_tree_to_syntax_node(tree.root_node(), &source_code);
        let diagnostics = self.collect_diagnostics(&tree, &source_code);

        Ok(SyntaxTree {
            root,
            source_code,
            language,
            diagnostics,
        })
    }

    /// Convert tree-sitter node to our SyntaxNode
    pub fn convert_tree_to_syntax_node(
        &self,
        node: tree_sitter::Node,
        source_code: &str,
    ) -> SyntaxNode {
        let start = node.start_position();
        let end = node.end_position();

        // First, convert all children sequentially so we can compute leading sibling comments
        let mut converted_children: Vec<SyntaxNode> = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            // Gather trailing run of comment siblings immediately preceding this child
            let mut leading_comments: Vec<String> = Vec::new();
            for prev in converted_children.iter().rev() {
                let k = prev.kind.to_lowercase();
                if k.contains("comment") {
                    leading_comments.push(prev.text.trim().to_string());
                } else {
                    break;
                }
            }
            leading_comments.reverse();

            // Convert current child
            let mut converted = self.convert_tree_to_syntax_node(child, source_code);
            converted.leading_comments = leading_comments;
            converted_children.push(converted);
        }

        SyntaxNode {
            kind: node.kind().to_string(),
            start_position: Position {
                row: start.row,
                column: start.column,
                byte_offset: node.start_byte(),
            },
            end_position: Position {
                row: end.row,
                column: end.column,
                byte_offset: node.end_byte(),
            },
            text: source_code[node.start_byte()..node.end_byte()].to_string(),
            children: converted_children,
            named_children: self.collect_named_children(node, source_code),
            leading_comments: Vec::new(),
        }
    }

    /// Collect named children for easier access
    fn collect_named_children(
        &self,
        node: tree_sitter::Node,
        source_code: &str,
    ) -> HashMap<String, Vec<SyntaxNode>> {
        let mut named_children = HashMap::new();

        for child in node.named_children(&mut node.walk()) {
            let kind = child.kind().to_string();
            let syntax_node = self.convert_tree_to_syntax_node(child, source_code);

            named_children
                .entry(kind)
                .or_insert_with(Vec::new)
                .push(syntax_node);
        }

        named_children
    }

    /// Collect diagnostics from the parsed tree
    pub fn collect_diagnostics(&self, tree: &Tree, _source_code: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Basic diagnostics collection - can be extended with more sophisticated analysis
        if tree.root_node().has_error() {
            diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Error,
                message: "Syntax error detected in code".to_string(),
                position: Position {
                    row: 0,
                    column: 0,
                    byte_offset: 0,
                },
                node_kind: "root".to_string(),
            });
        }

        diagnostics
    }

    /// Get parser statistics
    pub fn get_parser_stats(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();
        stats.insert(
            "supported_languages".to_string(),
            self.supported_languages.len(),
        );
        stats
    }

    pub fn analyze_file_with_tree_sitter(
        &mut self,
        file_path: &std::path::Path,
        source_code: &str,
    ) -> Result<CodeAnalysis> {
        let extension = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase());

        let is_swift_path = extension
            .as_deref()
            .map(|ext| ext == "swift")
            .unwrap_or(false);

        if is_swift_path && !cfg!(feature = "swift") {
            return Err(TreeSitterError::UnsupportedLanguage("Swift".to_string()).into());
        }

        let language = match self.detect_language_from_path(file_path) {
            Ok(language) => language,
            Err(err) => match self.detect_language_from_content(source_code) {
                Some(language) => language,
                None => return Err(err),
            },
        };

        self.current_file = file_path.to_string_lossy().to_string();

        let tree = self.parse(source_code, language.clone())?;

        // Extract actual symbols and dependencies
        let symbols = self.extract_symbols(&tree, source_code, language.clone())?;
        let dependencies = self.extract_dependencies(&tree, language.clone())?;
        let metrics = self.calculate_metrics(&tree, source_code)?;

        Ok(CodeAnalysis {
            file_path: self.current_file.clone(),
            language,
            symbols,
            dependencies,
            metrics,
            issues: vec![], // Would need to implement actual issue detection
            complexity: Default::default(), // Would need to implement actual complexity analysis
            structure: Default::default(), // Would need to implement actual structure analysis
        })
    }
}

/// Helper function to get tree-sitter language
fn get_language(language: LanguageSupport) -> Result<Language> {
    let lang = match language {
        LanguageSupport::Rust => tree_sitter_rust::LANGUAGE,
        LanguageSupport::Python => tree_sitter_python::LANGUAGE,
        LanguageSupport::JavaScript => tree_sitter_javascript::LANGUAGE,
        LanguageSupport::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        LanguageSupport::Go => tree_sitter_go::LANGUAGE,
        LanguageSupport::Java => tree_sitter_java::LANGUAGE,
        LanguageSupport::Bash => tree_sitter_bash::LANGUAGE,
        LanguageSupport::Swift => {
            #[cfg(feature = "swift")]
            {
                tree_sitter_swift::LANGUAGE
            }
            #[cfg(not(feature = "swift"))]
            {
                return Err(TreeSitterError::UnsupportedLanguage("Swift".to_string()).into());
            }
        }
    };
    Ok(lang.into())
}

impl std::fmt::Display for LanguageSupport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let language_name = match self {
            LanguageSupport::Rust => "Rust",
            LanguageSupport::Python => "Python",
            LanguageSupport::JavaScript => "JavaScript",
            LanguageSupport::TypeScript => "TypeScript",
            LanguageSupport::Go => "Go",
            LanguageSupport::Java => "Java",
            LanguageSupport::Bash => "Bash",
            LanguageSupport::Swift => "Swift",
        };
        write!(f, "{}", language_name)
    }
}

impl TreeSitterAnalyzer {
    pub fn detect_language_from_content(&self, content: &str) -> Option<LanguageSupport> {
        // Simple heuristic-based language detection
        if content.contains("fn ") && content.contains("{") && content.contains("}") {
            Some(LanguageSupport::Rust)
        } else if content.contains("def ") && content.contains(":") && !content.contains("{") {
            Some(LanguageSupport::Python)
        } else if content.contains("function") && content.contains("{") && content.contains("}") {
            Some(LanguageSupport::JavaScript)
        } else if content.starts_with("#!/bin/bash")
            || content.starts_with("#!/usr/bin/env bash")
            || content.starts_with("#!/bin/sh")
            || content.starts_with("#!/usr/bin/env sh")
            || content.contains("#!/usr/bin/env bash")
            || content.contains("#!/usr/bin/env sh")
        {
            Some(LanguageSupport::Bash)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn create_test_analyzer() -> TreeSitterAnalyzer {
        TreeSitterAnalyzer::new().expect("Failed to create analyzer")
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = create_test_analyzer();
        assert!(
            analyzer
                .supported_languages
                .contains(&LanguageSupport::Rust)
        );
        assert!(
            analyzer
                .supported_languages
                .contains(&LanguageSupport::Python)
        );
    }

    #[test]
    fn test_language_detection_from_path() {
        let analyzer = create_test_analyzer();

        // Test basic file extensions
        match analyzer.detect_language_from_path(Path::new("main.rs")) {
            Ok(lang) => assert_eq!(lang, LanguageSupport::Rust),
            Err(e) => panic!("Expected Rust language, got error: {}", e),
        }

        match analyzer.detect_language_from_path(Path::new("script.py")) {
            Ok(lang) => assert_eq!(lang, LanguageSupport::Python),
            Err(e) => panic!("Expected Python language, got error: {}", e),
        }

        // Test unknown extension should return error
        assert!(
            analyzer
                .detect_language_from_path(Path::new("file.unknown"))
                .is_err()
        );
    }

    #[test]
    fn test_language_detection_from_content() {
        let analyzer = create_test_analyzer();

        // Test Rust content
        let rust_code = r#"fn main() { println!("Hello, world!"); let x = 42; }"#;
        assert_eq!(
            analyzer.detect_language_from_content(rust_code),
            Some(LanguageSupport::Rust)
        );

        // Test Python content
        let python_code = r#"def main(): print("Hello, world!"); x = 42"#;
        assert_eq!(
            analyzer.detect_language_from_content(python_code),
            Some(LanguageSupport::Python)
        );

        // Test unknown content
        let unknown_code = "This is not code just plain text.";
        assert_eq!(analyzer.detect_language_from_content(unknown_code), None);
    }

    #[test]
    fn test_parse_rust_code() {
        let mut analyzer = create_test_analyzer();

        let rust_code = r#"fn main() { println!("Hello, world!"); let x = 42; }"#;

        let result = analyzer.parse(rust_code, LanguageSupport::Rust);
        assert!(result.is_ok());

        let tree = result.unwrap();
        assert!(!tree.root_node().has_error());
    }

    #[cfg(feature = "swift")]
    #[test]
    fn test_parse_swift_code() {
        let mut analyzer = create_test_analyzer();
        let swift_code = "print(\"Hello, World!\")\n";
        let result = analyzer.parse(swift_code, LanguageSupport::Swift);
        assert!(result.is_ok());
        let tree = result.unwrap();
        assert!(!tree.root_node().has_error());
    }
}
