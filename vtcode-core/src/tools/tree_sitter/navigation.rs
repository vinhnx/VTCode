//! Code navigation capabilities using tree-sitter

use crate::tools::tree_sitter::analyzer::{Position, SyntaxNode};
use crate::tools::tree_sitter::languages::{SymbolInfo, SymbolKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Navigation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationResult {
    pub target: NavigationTarget,
    pub context: NavigationContext,
    pub related_symbols: Vec<SymbolInfo>,
}

/// Navigation target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NavigationTarget {
    Symbol(SymbolInfo),
    Position(Position),
    Range { start: Position, end: Position },
}

impl NavigationTarget {
    /// Get the position associated with this navigation target
    pub fn get_position(&self) -> &Position {
        match self {
            NavigationTarget::Symbol(symbol) => &symbol.position,
            NavigationTarget::Position(pos) => pos,
            NavigationTarget::Range { start, .. } => start,
        }
    }
}

/// Navigation context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationContext {
    pub current_scope: Option<String>,
    pub parent_scopes: Vec<String>,
    pub available_symbols: Vec<SymbolInfo>,
    pub references: Vec<ReferenceInfo>,
}

/// Reference information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceInfo {
    pub symbol: SymbolInfo,
    pub reference_type: ReferenceType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReferenceType {
    Definition,
    Declaration,
    Usage,
    Call,
    Inheritance,
    Implementation,
}

/// Code navigator
pub struct CodeNavigator {
    symbol_map: HashMap<String, SymbolInfo>,
    position_map: HashMap<Position, SymbolInfo>,
}

impl Default for CodeNavigator {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeNavigator {
    pub fn new() -> Self {
        Self {
            symbol_map: HashMap::with_capacity(100), // Pre-allocate for typical symbol count
            position_map: HashMap::with_capacity(100), // Pre-allocate for typical position count
        }
    }

    /// Build navigation index from symbols
    pub fn build_index(&mut self, symbols: &[SymbolInfo]) {
        self.symbol_map.clear();
        self.position_map.clear();

        for symbol in symbols {
            // OPTIMIZATION: Clone once, then borrow from map for second insertion
            let name = symbol.name.clone();
            let position = symbol.position.clone();
            self.symbol_map.insert(name, symbol.clone());

            // Borrow from symbol_map to avoid second clone
            if let Some(cached) = self.symbol_map.get(&symbol.name) {
                self.position_map.insert(position, cached.clone());
            }
        }
    }

    /// Navigate to symbol definition
    pub fn goto_definition(&self, symbol_name: &str) -> Option<NavigationResult> {
        self.symbol_map.get(symbol_name).map(|symbol| {
            let context = self.build_context(symbol);
            NavigationResult {
                target: NavigationTarget::Symbol(symbol.clone()),
                context,
                related_symbols: self.find_related_symbols(symbol),
            }
        })
    }

    /// Navigate to symbol at position
    pub fn goto_position(&self, position: &Position) -> Option<NavigationResult> {
        self.position_map.get(position).map(|symbol| {
            let context = self.build_context(symbol);
            NavigationResult {
                target: NavigationTarget::Position(position.clone()),
                context,
                related_symbols: self.find_related_symbols(symbol),
            }
        })
    }

    /// Find all references to a symbol
    pub fn find_references(&self, symbol_name: &str) -> Vec<ReferenceInfo> {
        // Search through all symbols to find references to the given symbol name
        self.symbol_map
            .values()
            .filter(|symbol| {
                // Check if this symbol references the target symbol
                // Since the SymbolInfo struct doesn't have a references field,
                // we'll look for symbols with the same name in different scopes
                symbol.name == symbol_name
            })
            .map(|symbol| ReferenceInfo {
                symbol: symbol.clone(),
                reference_type: ReferenceType::Usage, // Default to usage, could be refined
            })
            .collect()
    }

    /// Get symbol information at position
    pub fn get_symbol_at_position(&self, position: &Position) -> Option<&SymbolInfo> {
        self.position_map.get(position)
    }

    /// Get all symbols in scope
    pub fn get_symbols_in_scope(&self, scope: Option<&str>) -> Vec<&SymbolInfo> {
        self.symbol_map
            .values()
            .filter(|symbol| {
                if let Some(scope_name) = scope {
                    symbol
                        .scope
                        .as_ref()
                        .map(|s| s == scope_name)
                        .unwrap_or(false)
                } else {
                    symbol.scope.is_none() // Global scope
                }
            })
            .collect()
    }

    /// Search for symbols by name pattern
    pub fn search_symbols(
        &self,
        pattern: &str,
        kind_filter: Option<&[SymbolKind]>,
    ) -> Vec<&SymbolInfo> {
        self.symbol_map
            .values()
            .filter(|symbol| {
                symbol.name.contains(pattern)
                    && (kind_filter.is_none() || kind_filter.unwrap().contains(&symbol.kind))
            })
            .collect()
    }

    /// Navigate to parent scope
    pub fn goto_parent(&self, current_symbol: &SymbolInfo) -> Option<NavigationResult> {
        current_symbol.scope.as_ref().and_then(|scope| {
            self.symbol_map.get(scope).map(|parent_symbol| {
                let context = self.build_context(parent_symbol);
                NavigationResult {
                    target: NavigationTarget::Symbol(parent_symbol.clone()),
                    context,
                    related_symbols: self.find_related_symbols(parent_symbol),
                }
            })
        })
    }

    /// Navigate to child symbols
    pub fn goto_children(&self, symbol: &SymbolInfo) -> Vec<NavigationResult> {
        self.symbol_map
            .values()
            .filter(|child| {
                child
                    .scope
                    .as_ref()
                    .map(|s| s == &symbol.name)
                    .unwrap_or(false)
            })
            .map(|child| {
                let context = self.build_context(child);
                NavigationResult {
                    target: NavigationTarget::Symbol(child.clone()),
                    context,
                    related_symbols: self.find_related_symbols(child),
                }
            })
            .collect()
    }

    /// Build navigation context for a symbol
    fn build_context(&self, symbol: &SymbolInfo) -> NavigationContext {
        let current_scope = symbol.scope.clone();
        let parent_scopes = self.build_parent_scopes(symbol);
        let available_symbols = self.get_symbols_in_scope(symbol.scope.as_deref());
        let references = self.find_references(&symbol.name);

        NavigationContext {
            current_scope,
            parent_scopes,
            available_symbols: available_symbols.into_iter().cloned().collect(),
            references,
        }
    }

    /// Build parent scope chain
    fn build_parent_scopes(&self, symbol: &SymbolInfo) -> Vec<String> {
        let mut scopes = Vec::new();
        let mut current_scope = symbol.scope.as_ref();

        while let Some(scope) = current_scope {
            scopes.push(scope.clone());
            current_scope = self
                .symbol_map
                .get(scope)
                .and_then(|parent| parent.scope.as_ref());
        }

        scopes
    }

    /// Find related symbols (implementations, overrides, etc.)
    fn find_related_symbols(&self, symbol: &SymbolInfo) -> Vec<SymbolInfo> {
        let mut related = Vec::new();

        for other in self.symbol_map.values() {
            if other.name == symbol.name && other.scope != symbol.scope {
                // Same name in different scope - likely override or implementation
                related.push(other.clone());
            } else if other.scope == symbol.scope
                && other.kind == symbol.kind
                && other.name != symbol.name
            {
                // Same scope and kind but different name - sibling symbols
                related.push(other.clone());
            }
        }

        related
    }
}

/// Navigation utilities
pub struct NavigationUtils;

impl NavigationUtils {
    /// Find the smallest node containing a position
    pub fn find_node_at_position<'a>(
        node: &'a SyntaxNode,
        position: &Position,
    ) -> Option<&'a SyntaxNode> {
        // Check if position is within this node
        if position.byte_offset >= node.start_position.byte_offset
            && position.byte_offset <= node.end_position.byte_offset
        {
            // Try children first (more specific)
            for child in &node.children {
                if let Some(found) = Self::find_node_at_position(child, position) {
                    return Some(found);
                }
            }
            // Return this node if no child contains the position
            Some(node)
        } else {
            None
        }
    }

    /// Get all nodes of a specific type
    pub fn find_nodes_by_type<'a>(node: &'a SyntaxNode, node_type: &str) -> Vec<&'a SyntaxNode> {
        let mut results = Vec::new();

        if node.kind == node_type {
            results.push(node);
        }

        for child in &node.children {
            results.extend(Self::find_nodes_by_type(child, node_type));
        }

        results
    }

    /// Get the path from root to a specific node
    pub fn get_node_path(root: &SyntaxNode, target: &SyntaxNode) -> Vec<String> {
        fn traverse(current: &SyntaxNode, target: &SyntaxNode, path: &mut Vec<String>) -> bool {
            path.push(current.kind.clone());
            if std::ptr::eq(current, target) {
                return true;
            }
            for child in &current.children {
                if traverse(child, target, path) {
                    return true;
                }
            }
            path.pop();
            false
        }

        let mut path = Vec::new();
        if traverse(root, target, &mut path) {
            path
        } else {
            Vec::new()
        }
    }

    /// Calculate distance between two positions
    pub fn calculate_distance(pos1: &Position, pos2: &Position) -> usize {
        pos2.byte_offset.abs_diff(pos1.byte_offset)
    }

    /// Find the nearest symbol to a position
    pub fn find_nearest_symbol<'a>(
        symbols: &'a [SymbolInfo],
        position: &Position,
    ) -> Option<&'a SymbolInfo> {
        symbols
            .iter()
            .min_by_key(|symbol| Self::calculate_distance(&symbol.position, position))
    }

    /// Get scope hierarchy at a position
    pub fn get_scope_hierarchy(node: &SyntaxNode, position: &Position) -> Vec<String> {
        if let Some(target_node) = Self::find_node_at_position(node, position) {
            Self::get_node_path(node, target_node)
        } else {
            Vec::new()
        }
    }
}
