//! Code refactoring capabilities using tree-sitter

use crate::tools::tree_sitter::analyzer::{Position, SyntaxNode, SyntaxTree};
use crate::tools::tree_sitter::languages::{SymbolInfo, SymbolKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Refactoring operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactoringOperation {
    pub kind: RefactoringKind,
    pub description: String,
    pub changes: Vec<CodeChange>,
    pub preview: Vec<String>,
}

/// Type of refactoring operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RefactoringKind {
    Rename,
    ExtractFunction,
    ExtractVariable,
    InlineFunction,
    MoveFunction,
    ChangeSignature,
    AddParameter,
    RemoveParameter,
    ReorderParameters,
    AddDocumentation,
    RemoveUnused,
    SimplifyCondition,
    ExtractConstant,
}

/// Code change specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeChange {
    pub file_path: String,
    pub old_range: TextRange,
    pub new_text: String,
    pub description: String,
}

/// Text range specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextRange {
    pub start: Position,
    pub end: Position,
}

/// Refactoring result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactoringResult {
    pub success: bool,
    pub operations: Vec<RefactoringOperation>,
    pub conflicts: Vec<RefactoringConflict>,
    pub preview: String,
}

/// Refactoring conflict
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactoringConflict {
    pub kind: ConflictKind,
    pub message: String,
    pub position: Position,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictKind {
    NameConflict,
    ReferenceConflict,
    ScopeConflict,
    TypeConflict,
    ImportConflict,
}

/// Code refactoring engine
pub struct RefactoringEngine {
    #[allow(dead_code)]
    analysis_cache: HashMap<String, SyntaxTree>,
}

impl RefactoringEngine {
    pub fn new() -> Self {
        Self {
            analysis_cache: HashMap::with_capacity(100), // Pre-allocate for cache
        }
    }

    /// Analyze refactoring possibilities for a symbol
    pub fn analyze_refactoring_options(
        &self,
        symbol: &SymbolInfo,
        tree: &SyntaxTree,
    ) -> Vec<RefactoringOperation> {
        let mut operations = Vec::new();

        match &symbol.kind {
            SymbolKind::Function | SymbolKind::Method => {
                operations.extend(self.analyze_function_refactoring(symbol, tree));
            }
            SymbolKind::Variable => {
                operations.extend(self.analyze_variable_refactoring(symbol, tree));
            }
            SymbolKind::Class | SymbolKind::Struct => {
                operations.extend(self.analyze_class_refactoring(symbol, tree));
            }
            _ => {}
        }

        operations
    }

    /// Analyze function-specific refactoring options
    fn analyze_function_refactoring(
        &self,
        symbol: &SymbolInfo,
        _tree: &SyntaxTree,
    ) -> Vec<RefactoringOperation> {
        let mut operations = Vec::new();

        // Extract function (if function is too long)
        if let Some(signature) = &symbol.signature {
            if signature.lines().count() > 20 {
                operations.push(RefactoringOperation {
                    kind: RefactoringKind::ExtractFunction,
                    description: format!(
                        "Extract parts of {} into separate functions",
                        symbol.name
                    ),
                    changes: vec![], // Would be populated with actual changes
                    preview: vec!["// Extracted function".to_string()],
                });
            }
        }

        // Rename function
        operations.push(RefactoringOperation {
            kind: RefactoringKind::Rename,
            description: format!("Rename function {}", symbol.name),
            changes: vec![],
            preview: vec![format!("fn new_name(/* parameters */) {{ /* body */ }}")],
        });

        // Add documentation
        if symbol.documentation.is_none() {
            operations.push(RefactoringOperation {
                kind: RefactoringKind::AddDocumentation,
                description: format!("Add documentation to function {}", symbol.name),
                changes: vec![],
                preview: vec![
                    "/// Function documentation".to_string(),
                    format!("fn {}(/* parameters */) {{ /* body */ }}", symbol.name),
                ],
            });
        }

        operations
    }

    /// Analyze variable-specific refactoring options
    fn analyze_variable_refactoring(
        &self,
        symbol: &SymbolInfo,
        _tree: &SyntaxTree,
    ) -> Vec<RefactoringOperation> {
        let mut operations = Vec::new();

        // Extract constant
        operations.push(RefactoringOperation {
            kind: RefactoringKind::ExtractConstant,
            description: format!("Extract {} into a named constant", symbol.name),
            changes: vec![],
            preview: vec![format!(
                "const {}: Type = value;",
                symbol.name.to_uppercase()
            )],
        });

        // Rename variable
        operations.push(RefactoringOperation {
            kind: RefactoringKind::Rename,
            description: format!("Rename variable {}", symbol.name),
            changes: vec![],
            preview: vec!["let new_name = value;".to_string()],
        });

        operations
    }

    /// Analyze class/struct-specific refactoring options
    fn analyze_class_refactoring(
        &self,
        symbol: &SymbolInfo,
        _tree: &SyntaxTree,
    ) -> Vec<RefactoringOperation> {
        let mut operations = Vec::new();

        // Rename class/struct
        operations.push(RefactoringOperation {
            kind: RefactoringKind::Rename,
            description: format!("Rename {} {}", symbol.kind_str(), symbol.name),
            changes: vec![],
            preview: vec![
                "struct NewName {".to_string(),
                "    // fields".to_string(),
                "}".to_string(),
            ],
        });

        operations
    }

    /// Generate refactoring preview
    pub fn generate_preview(&self, operation: &RefactoringOperation) -> String {
        let mut preview = format!("=== {} ===\n", operation.description);
        preview.push_str(&format!("Type: {:?}\n\n", operation.kind));

        preview.push_str("Preview:\n");
        for line in &operation.preview {
            preview.push_str(&format!("  {}\n", line));
        }

        if !operation.changes.is_empty() {
            preview.push_str("\nChanges:\n");
            for change in &operation.changes {
                preview.push_str(&format!(
                    "  {}: {} -> {}\n",
                    change.file_path,
                    change.old_range.start.row,
                    change.new_text.chars().take(50).collect::<String>()
                ));
            }
        }

        preview
    }

    /// Apply refactoring operation
    pub async fn apply_refactoring(
        &mut self,
        operation: &RefactoringOperation,
    ) -> Result<RefactoringResult, RefactoringError> {
        // Validate operation
        let conflicts = self.validate_operation(operation).await?;

        if !conflicts.is_empty() {
            return Ok(RefactoringResult {
                success: false,
                operations: vec![operation.clone()],
                conflicts,
                preview: self.generate_preview(operation),
            });
        }

        // Apply changes
        let mut applied_changes = Vec::new();
        for change in &operation.changes {
            self.apply_change(change).await?;
            applied_changes.push(change.clone());
        }

        Ok(RefactoringResult {
            success: true,
            operations: vec![operation.clone()],
            conflicts: vec![],
            preview: self.generate_preview(operation),
        })
    }

    /// Validate refactoring operation
    async fn validate_operation(
        &self,
        operation: &RefactoringOperation,
    ) -> Result<Vec<RefactoringConflict>, RefactoringError> {
        let mut conflicts = Vec::new();

        match operation.kind {
            RefactoringKind::Rename => {
                // Check for naming conflicts
                conflicts.extend(self.check_naming_conflicts(operation).await);
            }
            RefactoringKind::ExtractFunction => {
                // Check for scope conflicts
                conflicts.extend(self.check_scope_conflicts(operation));
            }
            _ => {}
        }

        Ok(conflicts)
    }

    /// Check for naming conflicts
    async fn check_naming_conflicts(
        &self,
        operation: &RefactoringOperation,
    ) -> Vec<RefactoringConflict> {
        let mut conflicts = Vec::new();

        if operation.kind != RefactoringKind::Rename {
            return conflicts;
        }

        if let Some(change) = operation.changes.first() {
            if let Ok(content) = tokio::fs::read_to_string(&change.file_path).await {
                if let Ok(re) =
                    regex::Regex::new(&format!(r"\b{}\b", regex::escape(&change.new_text)))
                {
                    for mat in re.find_iter(&content) {
                        if mat.start() != change.old_range.start.byte_offset {
                            conflicts.push(RefactoringConflict {
                                kind: ConflictKind::NameConflict,
                                message: format!(
                                    "name '{}' already exists in {}",
                                    change.new_text, change.file_path
                                ),
                                position: Position {
                                    row: 0,
                                    column: 0,
                                    byte_offset: mat.start(),
                                },
                                suggestion: Some("choose a different name".to_string()),
                            });
                            break;
                        }
                    }
                }
            }
        }

        conflicts
    }

    /// Check for scope conflicts
    fn check_scope_conflicts(&self, _operation: &RefactoringOperation) -> Vec<RefactoringConflict> {
        // This would implement scope analysis
        Vec::new()
    }

    /// Apply a single change
    async fn apply_change(&mut self, change: &CodeChange) -> Result<(), RefactoringError> {
        if change.old_range.start.byte_offset > change.old_range.end.byte_offset {
            return Err(RefactoringError::InvalidRange(format!(
                "Invalid range: start > end in {}",
                change.file_path
            )));
        }

        let mut content = tokio::fs::read_to_string(&change.file_path)
            .await
            .map_err(|e| RefactoringError::FileOperationError(e.to_string()))?;

        let start = change.old_range.start.byte_offset;
        let end = change.old_range.end.byte_offset;
        if end > content.len() {
            return Err(RefactoringError::InvalidRange(format!(
                "range exceeds file length in {}",
                change.file_path
            )));
        }

        content.replace_range(start..end, &change.new_text);
        tokio::fs::write(&change.file_path, content)
            .await
            .map_err(|e| RefactoringError::FileOperationError(e.to_string()))?;

        Ok(())
    }
}

/// Refactoring error
#[derive(Debug, thiserror::Error)]
pub enum RefactoringError {
    #[error("Invalid text range: {0}")]
    InvalidRange(String),

    #[error("File operation failed: {0}")]
    FileOperationError(String),

    #[error("Validation failed: {0}")]
    ValidationError(String),

    #[error("Conflict detected: {0}")]
    ConflictError(String),
}

/// Refactoring utilities
pub struct RefactoringUtils;

impl RefactoringUtils {
    /// Suggest function extraction opportunities
    pub fn suggest_function_extraction(tree: &SyntaxTree) -> Vec<FunctionExtractionSuggestion> {
        let mut suggestions = Vec::new();

        // Find long functions that could be extracted
        Self::analyze_long_functions(&tree.root, &mut suggestions);

        suggestions
    }

    /// Suggest variable extraction opportunities
    pub fn suggest_variable_extraction(tree: &SyntaxTree) -> Vec<VariableExtractionSuggestion> {
        let mut suggestions = Vec::new();

        // Find repeated expressions that could be extracted
        Self::analyze_repeated_expressions(&tree.root, &mut suggestions);

        suggestions
    }

    /// Suggest constant extraction opportunities
    pub fn suggest_constant_extraction(tree: &SyntaxTree) -> Vec<ConstantExtractionSuggestion> {
        let mut suggestions = Vec::new();

        // Find magic numbers and strings that could be constants
        Self::analyze_magic_values(&tree.root, &mut suggestions);

        suggestions
    }

    fn analyze_long_functions(
        node: &SyntaxNode,
        suggestions: &mut Vec<FunctionExtractionSuggestion>,
    ) {
        if node.kind.contains("function") || node.kind.contains("method") {
            // Check if function body is long
            let body_length = Self::calculate_node_size(node);
            if body_length > 50 {
                // Arbitrary threshold
                if let Some(name_node) = node
                    .named_children
                    .get("name")
                    .and_then(|children| children.first())
                {
                    suggestions.push(FunctionExtractionSuggestion {
                        function_name: name_node.text.clone(),
                        position: name_node.start_position.clone(),
                        body_size: body_length,
                        suggestion:
                            "Consider extracting parts of this function into smaller functions"
                                .to_string(),
                    });
                }
            }
        }

        for child in &node.children {
            Self::analyze_long_functions(child, suggestions);
        }
    }

    fn analyze_repeated_expressions(
        node: &SyntaxNode,
        suggestions: &mut Vec<VariableExtractionSuggestion>,
    ) {
        use std::collections::HashMap;
        let mut expr_map: HashMap<String, (usize, Position)> = HashMap::with_capacity(50); // Pre-allocate for typical expression count

        fn traverse(node: &SyntaxNode, map: &mut HashMap<String, (usize, Position)>) {
            if node.kind.contains("expression") {
                let entry = map
                    .entry(node.text.clone())
                    .or_insert((0, node.start_position.clone()));
                entry.0 += 1;
            }
            for child in &node.children {
                traverse(child, map);
            }
        }

        traverse(node, &mut expr_map);

        for (expr, (count, pos)) in expr_map {
            if count > 1 {
                suggestions.push(VariableExtractionSuggestion {
                    expression: expr,
                    position: pos,
                    occurrences: count,
                    suggestion: "Consider extracting this repeated expression into a variable"
                        .to_string(),
                });
            }
        }
    }

    fn analyze_magic_values(
        node: &SyntaxNode,
        suggestions: &mut Vec<ConstantExtractionSuggestion>,
    ) {
        fn traverse(node: &SyntaxNode, out: &mut Vec<ConstantExtractionSuggestion>) {
            if node.kind.contains("number") || node.kind.contains("string") {
                let val = node.text.trim();
                if val != "0" && val != "1" && !val.is_empty() {
                    let value_type = if node.kind.contains("string") {
                        "string"
                    } else {
                        "number"
                    };
                    out.push(ConstantExtractionSuggestion {
                        value: val.to_string(),
                        position: node.start_position.clone(),
                        value_type: value_type.to_string(),
                        suggestion: "Consider extracting this literal into a constant".to_string(),
                    });
                }
            }
            for child in &node.children {
                traverse(child, out);
            }
        }

        traverse(node, suggestions);
    }

    fn calculate_node_size(node: &SyntaxNode) -> usize {
        node.end_position.byte_offset - node.start_position.byte_offset
    }
}

/// Function extraction suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionExtractionSuggestion {
    pub function_name: String,
    pub position: Position,
    pub body_size: usize,
    pub suggestion: String,
}

/// Variable extraction suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableExtractionSuggestion {
    pub expression: String,
    pub position: Position,
    pub occurrences: usize,
    pub suggestion: String,
}

/// Constant extraction suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstantExtractionSuggestion {
    pub value: String,
    pub position: Position,
    pub value_type: String,
    pub suggestion: String,
}

impl SymbolInfo {
    fn kind_str(&self) -> &str {
        match &self.kind {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Class => "class",
            SymbolKind::Struct => "struct",
            SymbolKind::Variable => "variable",
            _ => "symbol",
        }
    }
}
