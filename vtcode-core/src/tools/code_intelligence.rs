//! Code Intelligence Tool using Tree-Sitter
//!
//! Provides code navigation features:
//! - Go to definition
//! - Find references
//! - Hover information
//! - Document symbols
//! - Workspace symbol search

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use crate::tools::file_search_bridge::{self, FileSearchConfig};
use crate::tools::tree_sitter::{
    CodeNavigator, LanguageAnalyzer, NavigationUtils, Position, SymbolInfo, TreeSitterAnalyzer,
};

/// Code intelligence operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CodeIntelligenceOperation {
    GotoDefinition,
    FindReferences,
    Hover,
    DocumentSymbol,
    WorkspaceSymbol,
    StatusCheck,
}

impl std::fmt::Display for CodeIntelligenceOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodeIntelligenceOperation::GotoDefinition => write!(f, "goto_definition"),
            CodeIntelligenceOperation::FindReferences => write!(f, "find_references"),
            CodeIntelligenceOperation::Hover => write!(f, "hover"),
            CodeIntelligenceOperation::DocumentSymbol => write!(f, "document_symbol"),
            CodeIntelligenceOperation::WorkspaceSymbol => write!(f, "workspace_symbol"),
            CodeIntelligenceOperation::StatusCheck => write!(f, "status_check"),
        }
    }
}

/// Input for code intelligence operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeIntelligenceInput {
    /// The operation to perform
    pub operation: CodeIntelligenceOperation,
    /// File path for the operation
    pub file_path: Option<String>,
    /// Line number (1-based, as used in editors)
    pub line: Option<usize>,
    /// Character/column number (1-based, as used in editors)
    pub character: Option<usize>,
    /// Query pattern for workspace symbol search
    pub query: Option<String>,
}

/// Location information in output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationInfo {
    pub file_path: String,
    pub line: usize,
    pub character: usize,
    pub symbol: Option<SymbolOutput>,
}

/// Symbol information in output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolOutput {
    pub name: String,
    pub kind: String,
    pub signature: Option<String>,
    pub documentation: Option<String>,
}

impl From<&SymbolInfo> for SymbolOutput {
    fn from(symbol: &SymbolInfo) -> Self {
        SymbolOutput {
            name: symbol.name.clone(),
            kind: format!("{:?}", symbol.kind),
            signature: symbol.signature.clone(),
            documentation: symbol.documentation.clone(),
        }
    }
}

/// Code intelligence result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeIntelligenceOutput {
    pub success: bool,
    pub operation: String,
    pub result: Option<CodeIntelligenceResult>,
    pub error: Option<String>,
}

/// Result payload for different operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CodeIntelligenceResult {
    Locations { locations: Vec<LocationInfo> },
    Symbols { symbols: Vec<SymbolOutput> },
    Hover { contents: HoverContents },
    Custom(Value),
}

/// Hover information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoverContents {
    pub name: String,
    pub kind: String,
    pub signature: Option<String>,
    pub documentation: Option<String>,
    pub scope: Option<String>,
}



/// Code Intelligence Tool
#[derive(Clone)]
pub struct CodeIntelligenceTool {
    workspace_root: PathBuf,
}

impl CodeIntelligenceTool {
    /// Create a new code intelligence tool
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
        }
    }

    /// Execute a code intelligence operation
    pub async fn execute(&self, args: Value) -> Result<Value> {
        let input: CodeIntelligenceInput = serde_json::from_value(args.clone())
            .with_context(|| "Failed to parse code intelligence input")?;

        // Handle status check operation separately
        if input.operation == CodeIntelligenceOperation::StatusCheck {
            return Ok(json!(CodeIntelligenceOutput {
                success: true,
                operation: input.operation.to_string(),
                result: Some(CodeIntelligenceResult::Custom(json!({
                    "status": "Tree-sitter based code analysis available"
                }))),
                error: None,
            }));
        }

        // Use Tree-Sitter for all code intelligence operations
        let result = match input.operation {
            CodeIntelligenceOperation::GotoDefinition => self.goto_definition(&input).await,
            CodeIntelligenceOperation::FindReferences => self.find_references(&input).await,
            CodeIntelligenceOperation::Hover => self.hover(&input).await,
            CodeIntelligenceOperation::DocumentSymbol => self.document_symbol(&input).await,
            CodeIntelligenceOperation::WorkspaceSymbol => self.workspace_symbol(&input).await,
            CodeIntelligenceOperation::StatusCheck => {
                // This case should not happen since we handle StatusCheck earlier
                return Ok(json!(CodeIntelligenceOutput {
                    success: true,
                    operation: input.operation.to_string(),
                    result: Some(CodeIntelligenceResult::Custom(json!({
                        "status": "LSP status check already handled"
                    }))),
                    error: None,
                }));
            }
        };

        match result {
            Ok(output) => serde_json::to_value(output)
                .with_context(|| "Failed to serialize code intelligence output"),
            Err(e) => Ok(json!(CodeIntelligenceOutput {
                success: false,
                operation: input.operation.to_string(),
                result: None,
                error: Some(e.to_string()),
            })),
        }
    }

    /// Go to symbol definition
    async fn goto_definition(
        &self,
        input: &CodeIntelligenceInput,
    ) -> Result<CodeIntelligenceOutput> {
        let file_path = input
            .file_path
            .as_ref()
            .with_context(|| "file_path is required for goto_definition")?;
        let line = input
            .line
            .with_context(|| "line is required for goto_definition")?;
        let character = input
            .character
            .with_context(|| "character is required for goto_definition")?;

        let full_path = self.resolve_path(file_path)?;
        let (symbols, _source_code) = self.parse_file_and_extract_symbols(&full_path).await?;

        // Convert 1-based line/character to 0-based for internal use
        let target_position = Position {
            row: line.saturating_sub(1),
            column: character.saturating_sub(1),
            byte_offset: 0, // Will be refined based on actual content
        };

        // Find the nearest symbol at the position
        let nearest_symbol = NavigationUtils::find_nearest_symbol(&symbols, &target_position);

        let locations = match nearest_symbol {
            Some(symbol) => {
                vec![LocationInfo {
                    file_path: file_path.clone(),
                    line: symbol.position.row + 1, // Convert back to 1-based
                    character: symbol.position.column + 1,
                    symbol: Some(SymbolOutput::from(symbol)),
                }]
            }
            None => vec![],
        };

        Ok(CodeIntelligenceOutput {
            success: true,
            operation: "goto_definition".to_string(),
            result: Some(CodeIntelligenceResult::Locations { locations }),
            error: None,
        })
    }

    /// Find all references to a symbol
    async fn find_references(
        &self,
        input: &CodeIntelligenceInput,
    ) -> Result<CodeIntelligenceOutput> {
        let file_path = input
            .file_path
            .as_ref()
            .with_context(|| "file_path is required for find_references")?;
        let line = input
            .line
            .with_context(|| "line is required for find_references")?;
        let character = input
            .character
            .with_context(|| "character is required for find_references")?;

        let full_path = self.resolve_path(file_path)?;
        let (symbols, _source_code) = self.parse_file_and_extract_symbols(&full_path).await?;

        // Convert 1-based to 0-based
        let target_position = Position {
            row: line.saturating_sub(1),
            column: character.saturating_sub(1),
            byte_offset: 0,
        };

        // Find the symbol at position first
        let target_symbol = NavigationUtils::find_nearest_symbol(&symbols, &target_position);

        let locations = match target_symbol {
            Some(symbol) => {
                // Build navigator and find references
                let mut navigator = CodeNavigator::new();
                navigator.build_index(&symbols);
                let references = navigator.find_references(&symbol.name);

                references
                    .iter()
                    .map(|ref_info| LocationInfo {
                        file_path: file_path.clone(),
                        line: ref_info.symbol.position.row + 1,
                        character: ref_info.symbol.position.column + 1,
                        symbol: Some(SymbolOutput::from(&ref_info.symbol)),
                    })
                    .collect()
            }
            None => vec![],
        };

        Ok(CodeIntelligenceOutput {
            success: true,
            operation: "find_references".to_string(),
            result: Some(CodeIntelligenceResult::Locations { locations }),
            error: None,
        })
    }

    /// Get hover information for a symbol
    async fn hover(&self, input: &CodeIntelligenceInput) -> Result<CodeIntelligenceOutput> {
        let file_path = input
            .file_path
            .as_ref()
            .with_context(|| "file_path is required for hover")?;
        let line = input.line.with_context(|| "line is required for hover")?;
        let character = input
            .character
            .with_context(|| "character is required for hover")?;

        let full_path = self.resolve_path(file_path)?;
        let (symbols, _source_code) = self.parse_file_and_extract_symbols(&full_path).await?;

        // Convert 1-based to 0-based
        let target_position = Position {
            row: line.saturating_sub(1),
            column: character.saturating_sub(1),
            byte_offset: 0,
        };

        let nearest_symbol = NavigationUtils::find_nearest_symbol(&symbols, &target_position);

        match nearest_symbol {
            Some(symbol) => Ok(CodeIntelligenceOutput {
                success: true,
                operation: "hover".to_string(),
                result: Some(CodeIntelligenceResult::Hover {
                    contents: HoverContents {
                        name: symbol.name.clone(),
                        kind: format!("{:?}", symbol.kind),
                        signature: symbol.signature.clone(),
                        documentation: symbol.documentation.clone(),
                        scope: symbol.scope.clone(),
                    },
                }),
                error: None,
            }),
            None => Ok(CodeIntelligenceOutput {
                success: true,
                operation: "hover".to_string(),
                result: Some(CodeIntelligenceResult::Hover {
                    contents: HoverContents {
                        name: String::new(),
                        kind: "unknown".to_string(),
                        signature: None,
                        documentation: None,
                        scope: None,
                    },
                }),
                error: None,
            }),
        }
    }

    /// Get all symbols in a document
    async fn document_symbol(
        &self,
        input: &CodeIntelligenceInput,
    ) -> Result<CodeIntelligenceOutput> {
        let file_path = input
            .file_path
            .as_ref()
            .with_context(|| "file_path is required for document_symbol")?;

        let full_path = self.resolve_path(file_path)?;
        let (symbols, _source_code) = self.parse_file_and_extract_symbols(&full_path).await?;

        let symbol_outputs: Vec<SymbolOutput> = symbols.iter().map(SymbolOutput::from).collect();

        Ok(CodeIntelligenceOutput {
            success: true,
            operation: "document_symbol".to_string(),
            result: Some(CodeIntelligenceResult::Symbols {
                symbols: symbol_outputs,
            }),
            error: None,
        })
    }

    /// Search for symbols across the workspace
    async fn workspace_symbol(
        &self,
        input: &CodeIntelligenceInput,
    ) -> Result<CodeIntelligenceOutput> {
        let query = input
            .query
            .as_ref()
            .with_context(|| "query is required for workspace_symbol")?;

        // Get source files in workspace
        let source_files = self.find_source_files().await?;

        let mut all_symbols: Vec<SymbolOutput> = Vec::new();
        let mut analyzer =
            TreeSitterAnalyzer::new().with_context(|| "Failed to create tree-sitter analyzer")?;

        for file_path in source_files.iter().take(100) {
            // Limit to 100 files for performance
            if let Ok((symbols, _)) = self
                .parse_file_with_analyzer(&mut analyzer, file_path)
                .await
            {
                // Filter symbols by query
                let matching_symbols: Vec<SymbolOutput> = symbols
                    .iter()
                    .filter(|s| s.name.to_lowercase().contains(&query.to_lowercase()))
                    .map(SymbolOutput::from)
                    .collect();
                all_symbols.extend(matching_symbols);
            }
        }

        // Limit results
        all_symbols.truncate(50);

        Ok(CodeIntelligenceOutput {
            success: true,
            operation: "workspace_symbol".to_string(),
            result: Some(CodeIntelligenceResult::Symbols {
                symbols: all_symbols,
            }),
            error: None,
        })
    }

    /// Resolve a file path relative to workspace
    fn resolve_path(&self, file_path: &str) -> Result<PathBuf> {
        let path = Path::new(file_path);
        let full_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        };

        // Validate path is within workspace
        let canonical = full_path
            .canonicalize()
            .with_context(|| format!("File not found: {}", file_path))?;

        let workspace_canonical = self
            .workspace_root
            .canonicalize()
            .with_context(|| "Failed to resolve workspace root")?;

        if !canonical.starts_with(&workspace_canonical) {
            anyhow::bail!("Path is outside workspace: {}", file_path);
        }

        Ok(canonical)
    }

    /// Parse a file and extract symbols
    async fn parse_file_and_extract_symbols(
        &self,
        file_path: &Path,
    ) -> Result<(Vec<SymbolInfo>, String)> {
        let mut analyzer =
            TreeSitterAnalyzer::new().with_context(|| "Failed to create tree-sitter analyzer")?;
        self.parse_file_with_analyzer(&mut analyzer, file_path)
            .await
    }

    /// Parse a file with a given analyzer
    async fn parse_file_with_analyzer(
        &self,
        analyzer: &mut TreeSitterAnalyzer,
        file_path: &Path,
    ) -> Result<(Vec<SymbolInfo>, String)> {
        let source_code = tokio::fs::read_to_string(file_path)
            .await
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        let language = analyzer
            .detect_language_from_path(file_path)
            .with_context(|| format!("Unsupported language for: {}", file_path.display()))?;

        let tree = analyzer
            .parse(&source_code, language)
            .with_context(|| format!("Failed to parse: {}", file_path.display()))?;

        let syntax_tree = crate::tools::tree_sitter::SyntaxTree {
            root: analyzer.convert_tree_to_syntax_node(tree.root_node(), &source_code),
            source_code: source_code.clone(),
            language,
            diagnostics: vec![],
        };

        let lang_analyzer = LanguageAnalyzer::new(&language);
        let symbols = lang_analyzer.extract_symbols(&syntax_tree);

        Ok((symbols, source_code))
    }

    /// Find source files in workspace
    async fn find_source_files(&self) -> Result<Vec<PathBuf>> {
        let extensions = [
            "rs", "py", "js", "ts", "tsx", "jsx", "go", "java", "sh", "bash",
        ];

        // Use file search bridge for efficient parallel traversal with .gitignore support
        let mut config = FileSearchConfig::new("".to_string(), self.workspace_root.clone())
            .with_limit(500) // Reasonable limit for code intelligence
            .respect_gitignore(true);

        // Exclude common directories that shouldn't be analyzed
        for pattern in &[
            "node_modules/**",
            "target/**",
            "build/**",
            "dist/**",
            ".git/**",
            ".vscode/**",
            ".cursor/**",
        ] {
            config = config.exclude(pattern.to_string());
        }

        match file_search_bridge::search_files(config, None) {
            Ok(results) => {
                // Filter by supported source file extensions
                let files = results
                    .matches
                    .into_iter()
                    .filter(|m| {
                        extensions.iter().any(|ext| {
                            m.path.ends_with(&format!(".{}", ext)) || m.path.ends_with(ext)
                        })
                    })
                    .map(|m| PathBuf::from(&m.path))
                    .collect::<Vec<_>>();

                Ok(files)
            }
            Err(_) => {
                // Fallback to manual traversal if file search fails
                let mut files = Vec::new();
                let mut stack = vec![self.workspace_root.clone()];

                while let Some(dir) = stack.pop() {
                    if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
                        while let Ok(Some(entry)) = entries.next_entry().await {
                            let path = entry.path();
                            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                            // Skip hidden directories and common excludes
                            if file_name.starts_with('.')
                                || file_name == "node_modules"
                                || file_name == "target"
                                || file_name == "build"
                                || file_name == "dist"
                            {
                                continue;
                            }

                            if path.is_dir() {
                                stack.push(path);
                            } else if let Some(ext) = path.extension().and_then(|e| e.to_str())
                                && extensions.contains(&ext)
                            {
                                files.push(path);
                            }
                        }
                    }
                }

                Ok(files)
            }
        }
    }

    /// Get the tool name
    pub fn name() -> &'static str {
        "code_intelligence"
    }

    /// Get the tool description
    pub fn description() -> &'static str {
        "Code intelligence tool providing go-to-definition, find-references, hover, and symbol search using tree-sitter analysis"
    }

    /// Get the parameter schema
    pub fn parameter_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["goto_definition", "find_references", "hover", "document_symbol", "workspace_symbol"],
                    "description": "The code intelligence operation to perform"
                },
                "file_path": {
                    "type": "string",
                    "description": "Path to the file (required for goto_definition, find_references, hover, document_symbol)"
                },
                "line": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Line number (1-based, required for goto_definition, find_references, hover)"
                },
                "character": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Character/column number (1-based, required for goto_definition, find_references, hover)"
                },
                "query": {
                    "type": "string",
                    "description": "Search pattern for workspace_symbol operation"
                }
            },
            "required": ["operation"]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    /// Get the project root by finding the directory containing Cargo.toml
    fn find_project_root() -> PathBuf {
        let mut current = env::current_dir().unwrap();
        loop {
            if current.join("Cargo.toml").exists() {
                // Check if this is the workspace root (has vtcode-core subdirectory)
                if current.join("vtcode-core").exists() {
                    return current;
                }
            }
            if !current.pop() {
                // Fall back to current directory
                return env::current_dir().unwrap();
            }
        }
    }

    #[tokio::test]
    async fn test_document_symbol() {
        let workspace = find_project_root();
        let tool = CodeIntelligenceTool::new(workspace.clone());

        let file_path = workspace.join("vtcode-core/src/tools/code_intelligence.rs");
        if !file_path.exists() {
            // Skip test if file doesn't exist (e.g., running from different directory)
            return;
        }

        let input = json!({
            "operation": "document_symbol",
            "file_path": file_path.to_string_lossy()
        });

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output: CodeIntelligenceOutput = serde_json::from_value(result.unwrap()).unwrap();
        assert!(
            output.success,
            "Expected success but got error: {:?}",
            output.error
        );
        assert_eq!(output.operation, "document_symbol");
    }

    #[tokio::test]
    async fn test_goto_definition() {
        let workspace = find_project_root();
        let tool = CodeIntelligenceTool::new(workspace.clone());

        let file_path = workspace.join("vtcode-core/src/tools/code_intelligence.rs");
        if !file_path.exists() {
            // Skip test if file doesn't exist
            return;
        }

        let input = json!({
            "operation": "goto_definition",
            "file_path": file_path.to_string_lossy(),
            "line": 1,
            "character": 1
        });

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output: CodeIntelligenceOutput = serde_json::from_value(result.unwrap()).unwrap();
        assert!(
            output.success,
            "Expected success but got error: {:?}",
            output.error
        );
    }

    #[test]
    fn test_input_parsing() {
        let input: CodeIntelligenceInput = serde_json::from_value(json!({
            "operation": "hover",
            "file_path": "test.rs",
            "line": 10,
            "character": 5
        }))
        .unwrap();

        assert_eq!(input.operation, CodeIntelligenceOperation::Hover);
        assert_eq!(input.file_path, Some("test.rs".to_string()));
        assert_eq!(input.line, Some(10));
        assert_eq!(input.character, Some(5));
    }

    #[test]
    fn test_parameter_schema() {
        let schema = CodeIntelligenceTool::parameter_schema();
        assert!(schema.get("properties").is_some());
        assert!(schema.get("required").is_some());
    }
}
