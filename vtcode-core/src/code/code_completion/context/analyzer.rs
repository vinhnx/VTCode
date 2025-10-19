use super::CompletionContext;
use crate::tools::tree_sitter::TreeSitterAnalyzer;
// tree_sitter::Point import removed as it's not used

/// Context analyzer for understanding code context
pub struct ContextAnalyzer {
    tree_sitter: TreeSitterAnalyzer,
}

impl ContextAnalyzer {
    pub fn new() -> Self {
        Self {
            tree_sitter: TreeSitterAnalyzer::new().expect("Failed to initialize TreeSitter"),
        }
    }

    /// Analyze code context at the given position
    pub fn analyze(&mut self, source: &str, line: usize, column: usize) -> CompletionContext {
        let language = self.detect_language(source);
        let prefix = self.extract_prefix(source, line, column);

        let mut context = CompletionContext::new(line, column, prefix, language);
        context.scope = self.extract_scope(source, line, column);
        context.imports = self.extract_imports(source);
        context.recent_symbols = self.extract_recent_symbols(source, line);

        context
    }

    fn detect_language(&self, source: &str) -> String {
        // Try to detect language from content
        if let Some(language) = self.tree_sitter.detect_language_from_content(source) {
            return match language {
                crate::tools::tree_sitter::LanguageSupport::Rust => "rust".to_string(),
                crate::tools::tree_sitter::LanguageSupport::Python => "python".to_string(),
                crate::tools::tree_sitter::LanguageSupport::JavaScript => "javascript".to_string(),
                crate::tools::tree_sitter::LanguageSupport::TypeScript => "typescript".to_string(),
                crate::tools::tree_sitter::LanguageSupport::Go => "go".to_string(),
                crate::tools::tree_sitter::LanguageSupport::Java => "java".to_string(),
                crate::tools::tree_sitter::LanguageSupport::Bash => "bash".to_string(),
                crate::tools::tree_sitter::LanguageSupport::Swift => "swift".to_string(),
            };
        }

        // Default to rust if no language detected
        "rust".to_string()
    }

    fn extract_prefix(&self, source: &str, line: usize, column: usize) -> String {
        let lines: Vec<&str> = source.lines().collect();
        if line < lines.len() && column <= lines[line].len() {
            lines[line][..column].to_string()
        } else {
            String::new()
        }
    }

    fn extract_scope(&mut self, source: &str, line: usize, column: usize) -> Vec<String> {
        let language = self.detect_language(source);

        // Parse the source code
        let lang_support = match language.as_str() {
            "rust" => crate::tools::tree_sitter::LanguageSupport::Rust,
            "python" => crate::tools::tree_sitter::LanguageSupport::Python,
            "javascript" => crate::tools::tree_sitter::LanguageSupport::JavaScript,
            "typescript" => crate::tools::tree_sitter::LanguageSupport::TypeScript,
            "go" => crate::tools::tree_sitter::LanguageSupport::Go,
            "java" => crate::tools::tree_sitter::LanguageSupport::Java,
            "bash" | "sh" => crate::tools::tree_sitter::LanguageSupport::Bash,
            _ => crate::tools::tree_sitter::LanguageSupport::Rust,
        };

        // Try to parse the source code
        if let Ok(tree) = self.tree_sitter.parse(source, lang_support) {
            let root_node = tree.root_node();
            let mut scopes = Vec::new();

            // Find the node at the given line/column position
            if let Some(node) = Self::find_node_at_position(root_node, line, column) {
                // Walk up the tree to collect scope information
                let mut current = Some(node);
                while let Some(n) = current {
                    let kind = n.kind();

                    // Add relevant scope information
                    if kind.contains("function") || kind.contains("method") {
                        scopes.push(format!("function:{}", kind));
                    } else if kind.contains("class") || kind.contains("struct") {
                        scopes.push(format!("class:{}", kind));
                    } else if kind.contains("module") || kind.contains("namespace") {
                        scopes.push(format!("module:{}", kind));
                    }

                    current = n.parent();
                }
            }

            scopes
        } else {
            vec![]
        }
    }

    /// Extract import statements from source code
    fn extract_imports(&mut self, source: &str) -> Vec<String> {
        let language = self.detect_language(source);

        // Parse the source code
        let lang_support = match language.as_str() {
            "rust" => crate::tools::tree_sitter::LanguageSupport::Rust,
            "python" => crate::tools::tree_sitter::LanguageSupport::Python,
            "javascript" => crate::tools::tree_sitter::LanguageSupport::JavaScript,
            "typescript" => crate::tools::tree_sitter::LanguageSupport::TypeScript,
            "go" => crate::tools::tree_sitter::LanguageSupport::Go,
            "java" => crate::tools::tree_sitter::LanguageSupport::Java,
            "bash" | "sh" => crate::tools::tree_sitter::LanguageSupport::Bash,
            _ => crate::tools::tree_sitter::LanguageSupport::Rust,
        };

        if let Ok(tree) = self.tree_sitter.parse(source, lang_support) {
            let root_node = tree.root_node();
            let mut imports = Vec::new();

            // Walk the tree to find import/require statements
            Self::extract_imports_recursive(root_node, source, &lang_support, &mut imports);

            imports
        } else {
            vec![]
        }
    }

    /// Recursively extract import statements from the syntax tree
    fn extract_imports_recursive(
        node: tree_sitter::Node,
        source: &str,
        language: &crate::tools::tree_sitter::LanguageSupport,
        imports: &mut Vec<String>,
    ) {
        let kind = node.kind();

        // Check for import statements based on language
        match language {
            crate::tools::tree_sitter::LanguageSupport::Rust => {
                if kind == "use_declaration" {
                    let import_text = &source[node.start_byte()..node.end_byte()];
                    imports.push(import_text.to_string());
                }
            }
            crate::tools::tree_sitter::LanguageSupport::Python => {
                if kind == "import_statement" || kind == "import_from_statement" {
                    let import_text = &source[node.start_byte()..node.end_byte()];
                    imports.push(import_text.to_string());
                }
            }
            crate::tools::tree_sitter::LanguageSupport::JavaScript
            | crate::tools::tree_sitter::LanguageSupport::TypeScript => {
                if kind == "import_statement" {
                    let import_text = &source[node.start_byte()..node.end_byte()];
                    imports.push(import_text.to_string());
                }
            }
            crate::tools::tree_sitter::LanguageSupport::Bash => {
                if kind == "command" {
                    let import_text = &source[node.start_byte()..node.end_byte()];
                    let trimmed = import_text.trim_start();
                    if trimmed.starts_with("source ") || trimmed.starts_with(". ") {
                        imports.push(import_text.to_string());
                    }
                }
            }
            _ => {
                // Generic approach for other languages
                if kind.contains("import") || kind.contains("require") {
                    let import_text = &source[node.start_byte()..node.end_byte()];
                    imports.push(import_text.to_string());
                }
            }
        }

        // Recursively process children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::extract_imports_recursive(child, source, language, imports);
        }
    }

    /// Extract recently used symbols from source code near the given line
    fn extract_recent_symbols(&mut self, source: &str, line: usize) -> Vec<String> {
        let language = self.detect_language(source);

        // Parse the source code
        let lang_support = match language.as_str() {
            "rust" => crate::tools::tree_sitter::LanguageSupport::Rust,
            "python" => crate::tools::tree_sitter::LanguageSupport::Python,
            "javascript" => crate::tools::tree_sitter::LanguageSupport::JavaScript,
            "typescript" => crate::tools::tree_sitter::LanguageSupport::TypeScript,
            "go" => crate::tools::tree_sitter::LanguageSupport::Go,
            "java" => crate::tools::tree_sitter::LanguageSupport::Java,
            "bash" | "sh" => crate::tools::tree_sitter::LanguageSupport::Bash,
            _ => crate::tools::tree_sitter::LanguageSupport::Rust,
        };

        // Try to parse the source code
        if let Ok(tree) = self.tree_sitter.parse(source, lang_support) {
            let _root_node = tree.root_node();
            let mut symbols = Vec::new();

            // Extract all symbols first
            if let Ok(extracted_symbols) =
                self.tree_sitter
                    .extract_symbols(&tree, source, lang_support)
            {
                // Filter symbols that appear before the given line
                for symbol in extracted_symbols {
                    if symbol.position.row < line {
                        symbols.push(symbol.name);
                    }
                }
            }

            // Return the last 10 symbols (most recent)
            if symbols.len() > 10 {
                symbols[symbols.len() - 10..].to_vec()
            } else {
                symbols
            }
        } else {
            vec![]
        }
    }

    /// Find the node that contains the given line/column position
    fn find_node_at_position<'a>(
        node: tree_sitter::Node<'a>,
        line: usize,
        column: usize,
    ) -> Option<tree_sitter::Node<'a>> {
        let start_pos = node.start_position();
        let end_pos = node.end_position();

        // Check if position is within this node
        if start_pos.row <= line && end_pos.row >= line {
            if start_pos.row == line && start_pos.column > column {
                return None;
            }
            if end_pos.row == line && end_pos.column < column {
                return None;
            }

            // Check children first (depth-first)
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if let Some(found) = Self::find_node_at_position(child, line, column) {
                    return Some(found);
                }
            }

            // If no child contains the position, this node does
            return Some(node);
        }

        None
    }
}

impl Default for ContextAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
