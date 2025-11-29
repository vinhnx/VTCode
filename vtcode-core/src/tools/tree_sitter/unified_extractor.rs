//! Unified symbol extraction framework to eliminate duplicate parsing logic
//!
//! This module provides a generic, language-agnostic symbol extraction system
//! that eliminates the massive code duplication in language-specific extraction.

use std::collections::HashMap;
use std::sync::Arc;
use tree_sitter::{Node, Tree};

use super::analyzer::{LanguageSupport, Position};
use super::languages::{SymbolInfo, SymbolKind};

/// Language-agnostic symbol extraction framework
pub struct UnifiedSymbolExtractor {
    /// Language-specific extraction patterns
    patterns: HashMap<LanguageSupport, LanguagePatterns>,
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
        let mut patterns = HashMap::new();

        // Initialize patterns for each supported language
        patterns.insert(LanguageSupport::Rust, Self::rust_patterns());
        patterns.insert(LanguageSupport::Python, Self::python_patterns());
        patterns.insert(LanguageSupport::JavaScript, Self::javascript_patterns());
        patterns.insert(LanguageSupport::TypeScript, Self::typescript_patterns());
        patterns.insert(LanguageSupport::Go, Self::go_patterns());
        patterns.insert(LanguageSupport::Java, Self::java_patterns());
        patterns.insert(LanguageSupport::Bash, Self::bash_patterns());
        patterns.insert(LanguageSupport::Swift, Self::swift_patterns());

        Self { patterns }
    }

    /// Extract symbols from a syntax tree using unified patterns
    pub fn extract_symbols(
        &self,
        syntax_tree: &Tree,
        source_code: &str,
        language: LanguageSupport,
    ) -> Vec<SymbolInfo> {
        let mut symbols = Vec::new();

        if let Some(patterns) = self.patterns.get(&language) {
            let root_node = syntax_tree.root_node();
            let mut scope_stack = Vec::new();

            self.extract_symbols_recursive(
                root_node,
                source_code,
                patterns,
                &mut scope_stack,
                &mut symbols,
            );
        }

        symbols
    }

    fn extract_symbols_recursive(
        &self,
        node: Node,
        source_code: &str,
        patterns: &LanguagePatterns,
        scope_stack: &mut Vec<String>,
        symbols: &mut Vec<SymbolInfo>,
    ) {
        let node_kind = node.kind();
        let current_scope = if scope_stack.is_empty() {
            None
        } else {
            Some(scope_stack.join("::"))
        };

        // Try to extract symbols from this node
        let mut symbol_extracted = false;

        // Check function patterns
        for pattern in &patterns.function_patterns {
            if pattern.node_kind == node_kind {
                if let Some(name) = self.extract_name(&node, source_code, &pattern.name_extraction) {
                    symbols.push(SymbolInfo {
                        name,
                        kind: pattern.symbol_kind,
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

                    // If this pattern creates a scope, push the name
                    if pattern.creates_scope {
                        scope_stack.push(name);
                    }
                    break;
                }
            }
        }

        // If not a function, check type patterns
        if !symbol_extracted {
            for pattern in &patterns.type_patterns {
                if pattern.node_kind == node_kind {
                    if let Some(name) = self.extract_name(&node, source_code, &pattern.name_extraction) {
                        symbols.push(SymbolInfo {
                            name,
                            kind: pattern.symbol_kind,
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
        }

        // If not a type, check variable patterns
        if !symbol_extracted {
            for pattern in &patterns.variable_patterns {
                if pattern.node_kind == node_kind {
                    if let Some(name) = self.extract_name(&node, source_code, &pattern.name_extraction) {
                        symbols.push(SymbolInfo {
                            name,
                            kind: pattern.symbol_kind,
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
            self.extract_symbols_recursive(child, source_code, patterns, scope_stack, symbols);
        }

        // Pop scope if we pushed one for this node
        if symbol_extracted && scope_stack.last().map_or(false, |s| {
            patterns.function_patterns.iter().any(|p| p.node_kind == node_kind && p.creates_scope) ||
            patterns.type_patterns.iter().any(|p| p.node_kind == node_kind && p.creates_scope)
        }) {
            scope_stack.pop();
        }
    }

    fn extract_name(&self, node: &Node, source_code: &str, extraction: &NameExtraction) -> Option<String> {
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
            NameExtraction::ChildByField(field_name) => {
                node.child_by_field_name(field_name)
                    .map(|child| source_code[child.byte_range()].to_string())
            }
            NameExtraction::NodeText => {
                Some(source_code[node.byte_range()].to_string())
            }
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
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Function,
                    creates_scope: false,
                },
                SymbolPattern {
                    node_kind: "method_definition",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Function,
                    creates_scope: false,
                },
            ],
            type_patterns: vec![
                SymbolPattern {
                    node_kind: "struct_item",
                    name_extraction: NameExtraction::ChildByType("type_identifier"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
                SymbolPattern {
                    node_kind: "enum_item",
                    name_extraction: NameExtraction::ChildByType("type_identifier"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
                SymbolPattern {
                    node_kind: "trait_item",
                    name_extraction: NameExtraction::ChildByType("type_identifier"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
            ],
            variable_patterns: vec![
                SymbolPattern {
                    node_kind: "constant_item",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Variable,
                    creates_scope: false,
                },
            ],
            module_patterns: vec![
                SymbolPattern {
                    node_kind: "mod_item",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Module,
                    creates_scope: true,
                },
            ],
        }
    }

    fn python_patterns() -> LanguagePatterns {
        LanguagePatterns {
            function_patterns: vec![
                SymbolPattern {
                    node_kind: "function_definition",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Function,
                    creates_scope: false,
                },
            ],
            type_patterns: vec![
                SymbolPattern {
                    node_kind: "class_definition",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
            ],
            variable_patterns: vec![
                SymbolPattern {
                    node_kind: "assignment",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Variable,
                    creates_scope: false,
                },
            ],
            module_patterns: vec![],
        }
    }

    fn javascript_patterns() -> LanguagePatterns {
        LanguagePatterns {
            function_patterns: vec![
                SymbolPattern {
                    node_kind: "function_declaration",
                    name_extraction: NameExtraction::ChildByType("identifier"),
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
            type_patterns: vec![
                SymbolPattern {
                    node_kind: "class_declaration",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Type,
                    creates_scope: false,
                },
            ],
            variable_patterns: vec![
                SymbolPattern {
                    node_kind: "variable_declarator",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Variable,
                    creates_scope: false,
                },
            ],
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
                name_extraction: NameExtraction::ChildByType("type_identifier"),
                symbol_kind: SymbolKind::Type,
                creates_scope: false,
            },
            SymbolPattern {
                node_kind: "type_alias_declaration",
                name_extraction: NameExtraction::ChildByType("type_identifier"),
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
            function_patterns: vec![
                SymbolPattern {
                    node_kind: "method_declaration",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Function,
                    creates_scope: false,
                },
            ],
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
            variable_patterns: vec![
                SymbolPattern {
                    node_kind: "field_declaration",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Variable,
                    creates_scope: false,
                },
            ],
            module_patterns: vec![
                SymbolPattern {
                    node_kind: "package_declaration",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Module,
                    creates_scope: true,
                },
            ],
        }
    }

    fn bash_patterns() -> LanguagePatterns {
        LanguagePatterns {
            function_patterns: vec![
                SymbolPattern {
                    node_kind: "function_definition",
                    name_extraction: NameExtraction::ChildByType("word"),
                    symbol_kind: SymbolKind::Function,
                    creates_scope: false,
                },
            ],
            type_patterns: vec![],
            variable_patterns: vec![
                SymbolPattern {
                    node_kind: "variable_assignment",
                    name_extraction: NameExtraction::ChildByType("word"),
                    symbol_kind: SymbolKind::Variable,
                    creates_scope: false,
                },
            ],
            module_patterns: vec![],
        }
    }

    fn swift_patterns() -> LanguagePatterns {
        LanguagePatterns {
            function_patterns: vec![
                SymbolPattern {
                    node_kind: "function_declaration",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Function,
                    creates_scope: false,
                },
            ],
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
            variable_patterns: vec![
                SymbolPattern {
                    node_kind: "property_declaration",
                    name_extraction: NameExtraction::ChildByType("identifier"),
                    symbol_kind: SymbolKind::Variable,
                    creates_scope: false,
                },
            ],
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
        assert!(extractor.patterns.contains_key(&LanguageSupport::JavaScript));
    }

    #[test]
    fn test_rust_patterns() {
        let patterns = UnifiedSymbolExtractor::rust_patterns();

        // Check function patterns
        assert!(patterns.function_patterns.iter().any(|p| p.node_kind == "function_item"));
        assert!(patterns.function_patterns.iter().any(|p| p.node_kind == "method_definition"));

        // Check type patterns
        assert!(patterns.type_patterns.iter().any(|p| p.node_kind == "struct_item"));
        assert!(patterns.type_patterns.iter().any(|p| p.node_kind == "enum_item"));
        assert!(patterns.type_patterns.iter().any(|p| p.node_kind == "trait_item"));
    }

    #[test]
    fn test_python_patterns() {
        let patterns = UnifiedSymbolExtractor::python_patterns();

        // Check function patterns
        assert!(patterns.function_patterns.iter().any(|p| p.node_kind == "function_definition"));

        // Check type patterns
        assert!(patterns.type_patterns.iter().any(|p| p.node_kind == "class_definition"));
    }
}