//! Unified symbol extraction framework to eliminate duplicate parsing logic
//!
//! This module provides a generic, language-agnostic symbol extraction system
//! that eliminates the massive code duplication in language-specific extraction.

use std::collections::HashMap;
use std::collections::HashSet;
use tree_sitter::{Node, Tree};

use super::analyzer::{LanguageSupport, Position};
use super::languages::{SymbolInfo, SymbolKind};

/// Language-agnostic symbol extraction framework
pub struct UnifiedSymbolExtractor {
    /// Language-specific extraction patterns
    patterns: HashMap<LanguageSupport, LanguagePatterns>,
    /// Cached node kinds for O(1) lookup instead of O(n) string comparisons
    cached_node_kinds: HashMap<LanguageSupport, NodeKindCache>,
}

/// Cached node kinds for O(1) lookup performance
#[derive(Clone)]
#[allow(dead_code)]
struct NodeKindCache {
    function_kinds: HashSet<&'static str>,
    type_kinds: HashSet<&'static str>,
    variable_kinds: HashSet<&'static str>,
    module_kinds: HashSet<&'static str>,
    scope_creating_kinds: HashSet<&'static str>,
}

/// Symbol extraction patterns for a specific language
#[derive(Clone)]
struct LanguagePatterns {
    /// Node kinds that represent functions/methods
    function_patterns: Vec<SymbolPattern>,
    /// Node kinds that represent types/classes
    type_patterns: Vec<SymbolPattern>,
    /// Node kinds that represent variables/constants
    variable_patterns: Vec<SymbolPattern>,
    /// Node kinds that represent modules/namespaces
    module_patterns: Vec<SymbolPattern>,
}

/// A pattern for extracting a symbol from a specific node type
#[derive(Clone)]
struct SymbolPattern {
    /// The tree-sitter node kind to match (e.g., "function_item", "class_definition")
    node_kind: &'static str,
    /// How to extract the symbol name from the node
    name_extraction: NameExtraction,
    /// The kind of symbol this represents
    symbol_kind: SymbolKind,
    /// Whether this symbol creates a new scope
    creates_scope: bool,
}

/// How to extract the name from a matched node
#[derive(Clone)]
#[allow(dead_code)]
enum NameExtraction {
    /// Find a child node with a specific type
    ChildByType(&'static str),
    /// Find a child node at a specific field
    ChildByField(&'static str),
    /// Use the node's own text content
    NodeText,
    /// Find multiple child nodes and combine them
    CombinedChildren(Vec<NameExtraction>),
}

impl UnifiedSymbolExtractor {
    pub fn new() -> Self {
        let mut patterns = HashMap::with_capacity(8); // Pre-allocate for 8 languages
        let mut cached_node_kinds = HashMap::with_capacity(8);

        // Initialize patterns for each supported language
        let rust_patterns = Self::rust_patterns();
        cached_node_kinds.insert(
            LanguageSupport::Rust,
            Self::build_node_kind_cache(&rust_patterns),
        );
        patterns.insert(LanguageSupport::Rust, rust_patterns);

        let python_patterns = Self::python_patterns();
        cached_node_kinds.insert(
            LanguageSupport::Python,
            Self::build_node_kind_cache(&python_patterns),
        );
        patterns.insert(LanguageSupport::Python, python_patterns);

        let js_patterns = Self::javascript_patterns();
        cached_node_kinds.insert(
            LanguageSupport::JavaScript,
            Self::build_node_kind_cache(&js_patterns),
        );
        patterns.insert(LanguageSupport::JavaScript, js_patterns);

        let ts_patterns = Self::typescript_patterns();
        cached_node_kinds.insert(
            LanguageSupport::TypeScript,
            Self::build_node_kind_cache(&ts_patterns),
        );
        patterns.insert(LanguageSupport::TypeScript, ts_patterns);

        let go_patterns = Self::go_patterns();
        cached_node_kinds.insert(
            LanguageSupport::Go,
            Self::build_node_kind_cache(&go_patterns),
        );
        patterns.insert(LanguageSupport::Go, go_patterns);

        let java_patterns = Self::java_patterns();
        cached_node_kinds.insert(
            LanguageSupport::Java,
            Self::build_node_kind_cache(&java_patterns),
        );
        patterns.insert(LanguageSupport::Java, java_patterns);

        let bash_patterns = Self::bash_patterns();
        cached_node_kinds.insert(
            LanguageSupport::Bash,
            Self::build_node_kind_cache(&bash_patterns),
        );
        patterns.insert(LanguageSupport::Bash, bash_patterns);

        let swift_patterns = Self::swift_patterns();
        cached_node_kinds.insert(
            LanguageSupport::Swift,
            Self::build_node_kind_cache(&swift_patterns),
        );
        patterns.insert(LanguageSupport::Swift, swift_patterns);

        Self {
            patterns,
            cached_node_kinds,
        }
    }

    /// Build node kind cache for O(1) lookup performance
    fn build_node_kind_cache(patterns: &LanguagePatterns) -> NodeKindCache {
        let mut function_kinds = HashSet::new();
        let mut type_kinds = HashSet::new();
        let mut variable_kinds = HashSet::new();
        let mut module_kinds = HashSet::new();
        let mut scope_creating_kinds = HashSet::new();

        // Pre-populate sets for O(1) lookup instead of O(n) iteration
        for pattern in &patterns.function_patterns {
            function_kinds.insert(pattern.node_kind);
            if pattern.creates_scope {
                scope_creating_kinds.insert(pattern.node_kind);
            }
        }
        for pattern in &patterns.type_patterns {
            type_kinds.insert(pattern.node_kind);
            if pattern.creates_scope {
                scope_creating_kinds.insert(pattern.node_kind);
            }
        }
        for pattern in &patterns.variable_patterns {
            variable_kinds.insert(pattern.node_kind);
        }
        for pattern in &patterns.module_patterns {
            module_kinds.insert(pattern.node_kind);
            if pattern.creates_scope {
                scope_creating_kinds.insert(pattern.node_kind);
            }
        }

        NodeKindCache {
            function_kinds,
            type_kinds,
            variable_kinds,
            module_kinds,
            scope_creating_kinds,
        }
    }

    /// Extract symbols from a syntax tree using unified patterns
    pub fn extract_symbols(
        &self,
        syntax_tree: &Tree,
        source_code: &str,
        language: LanguageSupport,
    ) -> Vec<SymbolInfo> {
        // Early return if language not supported
        if !self.patterns.contains_key(&language) {
            return Vec::new();
        }

        let mut symbols = Vec::new();
        let root_node = syntax_tree.root_node();
        let mut scope_stack = Vec::new();

        self.extract_symbols_recursive(
            root_node,
            source_code,
            language,
            &mut scope_stack,
            &mut symbols,
        );

        symbols
    }

    fn extract_symbols_recursive(
        &self,
        node: Node,
        source_code: &str,
        language: LanguageSupport,
        scope_stack: &mut Vec<String>,
        symbols: &mut Vec<SymbolInfo>,
    ) {
        let node_kind = node.kind();

        // Get cached node kinds for O(1) lookup instead of O(n) iteration
        let cache = match self.cached_node_kinds.get(&language) {
            Some(cache) => cache,
            None => return, // Early return if no cache for language
        };

        let current_scope = if scope_stack.is_empty() {
            None
        } else {
            Some(scope_stack.join("::"))
        };

        // Try to extract symbols from this node using O(1) cache lookup
        let mut symbol_extracted = false;
        let patterns = match self.patterns.get(&language) {
            Some(patterns) => patterns,
            None => return, // Early return if no patterns for language
        };

        // O(1) lookup using pre-cached sets instead of O(n) iteration
        if cache.function_kinds.contains(node_kind) {
            // Find the matching pattern efficiently
            for pattern in &patterns.function_patterns {
                if pattern.node_kind == node_kind {
                    if let Some(name) =
                        self.extract_name(&node, source_code, &pattern.name_extraction)
                    {
                        symbols.push(SymbolInfo {
                            name: name.clone(), // Clone only when we have a match
                            kind: pattern.symbol_kind.clone(),
                            position: Position {
                                row: node.start_position().row,
                                column: node.start_position().column,
                                byte_offset: node.start_byte(),
                            },
                            scope: current_scope.clone(),
                            signature: None,
                            documentation: None,
                        });
                        symbol_extracted = true;

                        if pattern.creates_scope {
                            scope_stack.push(name);
                        }
                        break;
                    }
                }
            }
        } else if cache.type_kinds.contains(node_kind) {
            // Check type patterns
            for pattern in &patterns.type_patterns {
                if pattern.node_kind == node_kind {
                    if let Some(name) =
                        self.extract_name(&node, source_code, &pattern.name_extraction)
                    {
                        symbols.push(SymbolInfo {
                            name: name.clone(),
                            kind: pattern.symbol_kind.clone(),
                            position: Position {
                                row: node.start_position().row,
                                column: node.start_position().column,
                                byte_offset: node.start_byte(),
                            },
                            scope: current_scope.clone(),
                            signature: None,
                            documentation: None,
                        });
                        symbol_extracted = true;

                        if pattern.creates_scope {
                            scope_stack.push(name);
                        }
                        break;
                    }
                }
            }
        } else if cache.variable_kinds.contains(node_kind) {
            // Check variable patterns
            for pattern in &patterns.variable_patterns {
                if pattern.node_kind == node_kind {
                    if let Some(name) =
                        self.extract_name(&node, source_code, &pattern.name_extraction)
                    {
                        symbols.push(SymbolInfo {
                            name, // No clone needed here
                            kind: pattern.symbol_kind.clone(),
                            position: Position {
                                row: node.start_position().row,
                                column: node.start_position().column,
                                byte_offset: node.start_byte(),
                            },
                            scope: current_scope.clone(),
                            signature: None,
                            documentation: None,
                        });
                        symbol_extracted = true;
                        break;
                    }
                }
            }
        }

        // Process children (even if we extracted a symbol from this node)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_symbols_recursive(child, source_code, language, scope_stack, symbols);
        }

        // Pop scope if we pushed one for this node - O(1) lookup using cache
        if symbol_extracted && cache.scope_creating_kinds.contains(node_kind) {
            scope_stack.pop();
        }
    }

    fn extract_name(
        &self,
        node: &Node,
        source_code: &str,
        extraction: &NameExtraction,
    ) -> Option<String> {
        match extraction {
            NameExtraction::ChildByType(child_type) => {
                node.child_by_field_name(child_type)
                    .or_else(|| {
                        // Fallback: find first child with matching type
                        let mut cursor = node.walk();
                        node.children(&mut cursor)
                            .find(|child| child.kind() == *child_type)
                    })
                    .map(|child| source_code[child.byte_range()].to_string())
            }
            NameExtraction::ChildByField(field_name) => node
                .child_by_field_name(field_name)
                .map(|child| source_code[child.byte_range()].to_string()),
            NameExtraction::NodeText => Some(source_code[node.byte_range()].to_string()),
            NameExtraction::CombinedChildren(children) => {
                let mut parts = Vec::new();
                for child_extraction in children {
                    if let Some(part) = self.extract_name(node, source_code, child_extraction) {
                        parts.push(part);
                    }
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join(""))
                }
            }
        }
    }

    // Language-specific pattern definitions

    fn rust_patterns() -> LanguagePatterns {
        LanguagePatterns {
            function_patterns: vec![
                SymbolPattern {
                    node_kind: "function_item",
                    name_extraction: NameExtraction::ChildByField("name"),
                    symbol_kind: SymbolKind::Function,
                    creates_scope: false,
                },
                SymbolPattern {
                    node_kind: "method_definition",
                    name_extraction: NameExtraction::ChildByField("name"),
                    symbol_kind: SymbolKind::Function,
                    creates_scope: false,
                },
            ],
            type_patterns: vec![
                SymbolPattern {
                    node_kind: "struct_item",
                    name_extraction: NameExtraction::ChildByField("name"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
                SymbolPattern {
                    node_kind: "enum_item",
                    name_extraction: NameExtraction::ChildByField("name"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
                SymbolPattern {
                    node_kind: "trait_item",
                    name_extraction: NameExtraction::ChildByField("name"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
            ],
            variable_patterns: vec![SymbolPattern {
                node_kind: "constant_item",
                name_extraction: NameExtraction::ChildByType("identifier"),
                symbol_kind: SymbolKind::Variable,
                creates_scope: false,
            }],
            module_patterns: vec![SymbolPattern {
                node_kind: "mod_item",
                name_extraction: NameExtraction::ChildByType("identifier"),
                symbol_kind: SymbolKind::Module,
                creates_scope: true,
            }],
        }
    }

    fn python_patterns() -> LanguagePatterns {
        LanguagePatterns {
            function_patterns: vec![SymbolPattern {
                node_kind: "function_definition",
                name_extraction: NameExtraction::ChildByField("name"),
                symbol_kind: SymbolKind::Function,
                creates_scope: false,
            }],
            type_patterns: vec![SymbolPattern {
                node_kind: "class_definition",
                name_extraction: NameExtraction::ChildByField("name"),
                symbol_kind: SymbolKind::Type,
                creates_scope: false,
            }],
            variable_patterns: vec![SymbolPattern {
                node_kind: "assignment",
                name_extraction: NameExtraction::ChildByType("identifier"),
                symbol_kind: SymbolKind::Variable,
                creates_scope: false,
            }],
            module_patterns: vec![],
        }
    }

    fn javascript_patterns() -> LanguagePatterns {
        LanguagePatterns {
            function_patterns: vec![
                SymbolPattern {
                    node_kind: "function_declaration",
                    name_extraction: NameExtraction::ChildByField("name"),
                    symbol_kind: SymbolKind::Function,
                    creates_scope: false,
                },
                SymbolPattern {
                    node_kind: "method_definition",
                    name_extraction: NameExtraction::ChildByType("property_identifier"),
                    symbol_kind: SymbolKind::Function,
                    creates_scope: false,
                },
            ],
            type_patterns: vec![SymbolPattern {
                node_kind: "class_declaration",
                name_extraction: NameExtraction::ChildByField("name"),
                symbol_kind: SymbolKind::Type,
                creates_scope: false,
            }],
            variable_patterns: vec![SymbolPattern {
                node_kind: "variable_declarator",
                name_extraction: NameExtraction::ChildByField("name"),
                symbol_kind: SymbolKind::Variable,
                creates_scope: false,
            }],
            module_patterns: vec![],
        }
    }

    fn typescript_patterns() -> LanguagePatterns {
        // TypeScript is similar to JavaScript but with additional type patterns
        let mut patterns = Self::javascript_patterns();

        // Add TypeScript-specific type patterns
        patterns.type_patterns.extend(vec![
            SymbolPattern {
                node_kind: "interface_declaration",
                name_extraction: NameExtraction::ChildByField("name"),
                symbol_kind: SymbolKind::Type,
                creates_scope: false,
            },
            SymbolPattern {
                node_kind: "type_alias_declaration",
                name_extraction: NameExtraction::ChildByField("name"),
                symbol_kind: SymbolKind::Type,
                creates_scope: false,
            },
        ]);

        patterns
    }

    fn go_patterns() -> LanguagePatterns {
        LanguagePatterns {
            function_patterns: vec![
                SymbolPattern {
                    node_kind: "function_declaration",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Function,
                    creates_scope: false,
                },
                SymbolPattern {
                    node_kind: "method_declaration",
                    name_extraction: NameExtraction::ChildByType("field_identifier"),
                    symbol_kind: SymbolKind::Function,
                    creates_scope: false,
                },
            ],
            type_patterns: vec![
                SymbolPattern {
                    node_kind: "type_declaration",
                    name_extraction: NameExtraction::ChildByType("type_identifier"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
                SymbolPattern {
                    node_kind: "struct_type",
                    name_extraction: NameExtraction::ChildByType("type_identifier"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
            ],
            variable_patterns: vec![],
            module_patterns: vec![],
        }
    }

    fn java_patterns() -> LanguagePatterns {
        LanguagePatterns {
            function_patterns: vec![SymbolPattern {
                node_kind: "method_declaration",
                name_extraction: NameExtraction::ChildByType("identifier"),
                symbol_kind: SymbolKind::Function,
                creates_scope: false,
            }],
            type_patterns: vec![
                SymbolPattern {
                    node_kind: "class_declaration",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
                SymbolPattern {
                    node_kind: "interface_declaration",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
            ],
            variable_patterns: vec![SymbolPattern {
                node_kind: "field_declaration",
                name_extraction: NameExtraction::ChildByType("identifier"),
                symbol_kind: SymbolKind::Variable,
                creates_scope: false,
            }],
            module_patterns: vec![SymbolPattern {
                node_kind: "package_declaration",
                name_extraction: NameExtraction::ChildByType("identifier"),
                symbol_kind: SymbolKind::Module,
                creates_scope: true,
            }],
        }
    }

    fn bash_patterns() -> LanguagePatterns {
        LanguagePatterns {
            function_patterns: vec![SymbolPattern {
                node_kind: "function_definition",
                name_extraction: NameExtraction::ChildByType("word"),
                symbol_kind: SymbolKind::Function,
                creates_scope: false,
            }],
            type_patterns: vec![],
            variable_patterns: vec![SymbolPattern {
                node_kind: "variable_assignment",
                name_extraction: NameExtraction::ChildByType("word"),
                symbol_kind: SymbolKind::Variable,
                creates_scope: false,
            }],
            module_patterns: vec![],
        }
    }

    fn swift_patterns() -> LanguagePatterns {
        LanguagePatterns {
            function_patterns: vec![SymbolPattern {
                node_kind: "function_declaration",
                name_extraction: NameExtraction::ChildByType("identifier"),
                symbol_kind: SymbolKind::Function,
                creates_scope: false,
            }],
            type_patterns: vec![
                SymbolPattern {
                    node_kind: "class_declaration",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
                SymbolPattern {
                    node_kind: "struct_declaration",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
                SymbolPattern {
                    node_kind: "protocol_declaration",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
            ],
            variable_patterns: vec![SymbolPattern {
                node_kind: "property_declaration",
                name_extraction: NameExtraction::ChildByType("identifier"),
                symbol_kind: SymbolKind::Variable,
                creates_scope: false,
            }],
            module_patterns: vec![],
        }
    }
}

impl Default for UnifiedSymbolExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_creation() {
        let extractor = UnifiedSymbolExtractor::new();

        // Test that patterns are created for each language
        assert!(extractor.patterns.contains_key(&LanguageSupport::Rust));
        assert!(extractor.patterns.contains_key(&LanguageSupport::Python));
        assert!(
            extractor
                .patterns
                .contains_key(&LanguageSupport::JavaScript)
        );
    }

    #[test]
    fn test_rust_patterns() {
        let patterns = UnifiedSymbolExtractor::rust_patterns();

        // Check function patterns
        assert!(
            patterns
                .function_patterns
                .iter()
                .any(|p| p.node_kind == "function_item")
        );
        assert!(
            patterns
                .function_patterns
                .iter()
                .any(|p| p.node_kind == "method_definition")
        );

        // Check type patterns
        assert!(
            patterns
                .type_patterns
                .iter()
                .any(|p| p.node_kind == "struct_item")
        );
        assert!(
            patterns
                .type_patterns
                .iter()
                .any(|p| p.node_kind == "enum_item")
        );
        assert!(
            patterns
                .type_patterns
                .iter()
                .any(|p| p.node_kind == "trait_item")
        );
    }

    #[test]
    fn test_python_patterns() {
        let patterns = UnifiedSymbolExtractor::python_patterns();

        // Check function patterns
        assert!(
            patterns
                .function_patterns
                .iter()
                .any(|p| p.node_kind == "function_definition")
        );

        // Check type patterns
        assert!(
            patterns
                .type_patterns
                .iter()
                .any(|p| p.node_kind == "class_definition")
        );
    }

    #[test]
    #[cfg(feature = "lang-rust")]
    fn test_extraction_rust() {
        let source = "fn my_func() {} struct MyStruct {}";
        let mut parser = tree_sitter::Parser::new();
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let extractor = UnifiedSymbolExtractor::new();
        let symbols = extractor.extract_symbols(&tree, source, LanguageSupport::Rust);

        assert_eq!(symbols.len(), 2);
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "my_func" && s.kind == SymbolKind::Function)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "MyStruct" && s.kind == SymbolKind::Type)
        );
    }
}
